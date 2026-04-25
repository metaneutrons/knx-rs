// SPDX-License-Identifier: GPL-3.0-only
// Reference tool for generating ApplicationProgram hash prebytes using the
// original ETS6 DLLs. Used to create golden test vectors for the Rust hasher.
//
// See README.md for setup instructions.

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
    static void Main(string[] args)
    {
        if (args.Length < 1)
        {
            Console.Error.WriteLine("Usage: ets-hash-reference <application.xml> [prebytes.bin]");
            Console.Error.WriteLine();
            Console.Error.WriteLine("Computes the ApplicationProgram MD5 hash and fingerprint using the");
            Console.Error.WriteLine("original ETS6 XmlSigning DLL. Optionally dumps the pre-MD5 byte stream.");
            return;
        }

        var xmlPath = args[0];
        var regRelevant = Information.RegistrationRelevantApplicationProgramElements;

        var asm = Assembly.LoadFrom("Knx.Ets.XmlSigning.dll");
        var genType = asm.GetType("Knx.Ets.XmlSigning.Signer.SignAndHashUtils+ChildElementHashGenerator");

        var settings = new XmlReaderSettings { IgnoreWhitespace = true, IgnoreComments = true };
        using var reader = XmlReader.Create(xmlPath, settings);

        // Navigate to the <ApplicationPrograms> element (parent of <ApplicationProgram>).
        while (reader.Read())
        {
            if (reader.NodeType == XmlNodeType.Element && reader.LocalName == "ApplicationPrograms")
                break;
        }

        var ctor = genType.GetConstructors(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)[0];
        var gen = ctor.Invoke(new object[]
        {
            reader, regRelevant, Naming.ApplicationPrograms,
            null, new DummyResolver(), null
        });

        genType.GetMethod("Generate")!.Invoke(gen, null);
        var hashBytes = (byte[])genType.GetProperty("HashBytes")!.GetValue(gen)!;

        using var md5 = MD5.Create();
        var md5Hash = md5.ComputeHash(hashBytes);

        Console.WriteLine($"MD5:         {BitConverter.ToString(md5Hash).Replace("-", "").ToLower()}");
        Console.WriteLine($"Fingerprint: {md5Hash[0]:X2}{md5Hash[15]:X2}");
        Console.WriteLine($"Prebytes:    {hashBytes.Length} bytes");

        if (args.Length > 1)
        {
            File.WriteAllBytes(args[1], hashBytes);
            Console.Error.WriteLine($"Wrote {hashBytes.Length} bytes to {args[1]}");
        }
    }
}
