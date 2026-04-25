// SPDX-License-Identifier: GPL-3.0-only
// Dumps per-element prebytes for debugging hash divergences.
// Writes one .bin file per direct child of <Static>, named by element.

using System;
using System.IO;
using System.Xml;
using System.Reflection;
using System.Security.Cryptography;
using Knx.Ets.Xml.RegistrationRelevanceInformation;

class DummyResolver : IDynamicValueResolverRegistry
{
    public Func<string, ElementValues, byte[]> Get(string elementName)
    {
        return (value, existingValues) =>
        {
            using var ms = new MemoryStream();
            using var bw = new BinaryWriter(ms);
            bw.Write(value ?? "$<null>$");
            return ms.ToArray();
        };
    }
}

class Program
{
    static Type _genType;
    static ConstructorInfo _ctor;
    static MethodInfo _generate;
    static PropertyInfo _hashBytes;
    static PropertyInfo _orderKey;

    static void Main(string[] args)
    {
        if (args.Length < 2)
        {
            Console.Error.WriteLine("Usage: ets-hash-debug <application.xml> <output-dir>");
            return;
        }

        var xmlPath = args[0];
        var outDir = args[1];
        Directory.CreateDirectory(outDir);

        var regRelevant = Information.RegistrationRelevantApplicationProgramElements;
        var asm = Assembly.LoadFrom("Knx.Ets.XmlSigning.dll");
        _genType = asm.GetType("Knx.Ets.XmlSigning.Signer.SignAndHashUtils+ChildElementHashGenerator");
        _ctor = _genType.GetConstructors(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)[0];
        _generate = _genType.GetMethod("Generate");
        _hashBytes = _genType.GetProperty("HashBytes");
        _orderKey = _genType.GetProperty("OrderKey");

        // First: dump the full prebytes
        var settings = new XmlReaderSettings { IgnoreWhitespace = true, IgnoreComments = true };
        using (var reader = XmlReader.Create(xmlPath, settings))
        {
            while (reader.Read())
                if (reader.NodeType == XmlNodeType.Element && reader.LocalName == "ApplicationPrograms")
                    break;

            var gen = _ctor.Invoke(new object[] { reader, regRelevant, Naming.ApplicationPrograms, null, new DummyResolver(), null });
            _generate.Invoke(gen, null);
            var bytes = (byte[])_hashBytes.GetValue(gen);

            File.WriteAllBytes(Path.Combine(outDir, "full.bin"), bytes);
            Console.WriteLine($"full.bin: {bytes.Length} bytes");
        }

        // Second: dump each child of ApplicationProgram separately
        // We do this by navigating to each child and creating a generator for it
        using (var reader = XmlReader.Create(xmlPath, settings))
        {
            // Find ApplicationProgram
            while (reader.Read())
                if (reader.NodeType == XmlNodeType.Element && reader.LocalName == "ApplicationProgram")
                    break;

            // Read children of ApplicationProgram
            int childIdx = 0;
            int depth = reader.Depth;
            while (reader.Read())
            {
                if (reader.Depth <= depth) break;
                if (reader.NodeType != XmlNodeType.Element) continue;
                if (reader.Depth != depth + 1) continue;

                var childName = reader.LocalName;
                var gen = _ctor.Invoke(new object[] { reader, regRelevant, Naming.ApplicationProgram, null, new DummyResolver(), null });
                _generate.Invoke(gen, null);
                var bytes = (byte[])_hashBytes.GetValue(gen);
                var key = _orderKey.GetValue(gen);

                var filename = $"{childIdx:D2}_{childName}.bin";
                if (bytes != null && bytes.Length > 0)
                {
                    File.WriteAllBytes(Path.Combine(outDir, filename), bytes);
                    Console.WriteLine($"{filename}: {bytes.Length} bytes, key={key}");
                }
                else
                {
                    Console.WriteLine($"{filename}: empty, key={key}");
                }
                childIdx++;
            }
        }
    }
}
