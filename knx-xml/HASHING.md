# KNX ApplicationProgram Hashing Algorithm

## Overview

The ApplicationProgram hash is an MD5 hash over the "registration relevant" attributes
of all XML elements in the ApplicationProgram tree. The hash determines the fingerprint
(last 4 hex chars) in the Application Program ID.

## Algorithm

1. Parse the ApplicationProgram XML with XmlReader
2. For each XML element encountered:
   a. Look up the element name in `RegistrationRelevantApplicationProgramElements`
   b. If found, extract the relevant attributes using their typed serializers
   c. Write the serialized bytes to a MemoryStream via BinaryWriter
3. MD5 hash the entire MemoryStream → 16 bytes
4. Fingerprint = `(hash[0] << 8) | hash[15]` → 4 hex chars (e.g. "DDAF")
5. New Application ID = old ID with last 4 chars replaced by fingerprint

## Typed Serializers

Each attribute has a specific type that determines how it's serialized:

| Type | C# BinaryWriter method | Bytes |
|------|----------------------|-------|
| `StringAttributeInfo` | `Write(string)` | varint length + UTF-8 |
| `UInt16AttributeInfo` | `Write(ushort)` | 2 bytes LE |
| `UInt32AttributeInfo` | `Write(uint)` | 4 bytes LE |
| `ByteAttributeInfo` | `Write(byte)` | 1 byte |
| `BoolAttributeInfo` | `Write(bool)` | 1 byte (0 or 1) |
| `Int32AttributeInfo` | `Write(int)` | 4 bytes LE |
| `Int64AttributeInfo` | `Write(long)` | 8 bytes LE |
| `DoubleAttributeInfo` | `Write(double)` | 8 bytes LE |
| `ApplProgIdAttributeInfo` | `Write(string)` | normalized ID (fingerprint → 0000) |

For missing/null attributes, `BinaryWriter.Write(AttributeInfo.NullString)` is used.

## Registration Relevant Elements (ApplicationProgram)

From `Knx.Ets.Xml.RegistrationRelevanceInformation.dll`:

```
Manufacturer: RefId(String, sort)
ApplicationProgram: Id(ApplProgId, sort), ApplicationNumber(UInt16), ApplicationVersion(Byte),
    ProgramType(String), MaskVersion(String), LoadProcedureStyle(String), PeiType(Byte),
    DynamicTableManagement(Bool), Linkable(Bool), OriginalManufacturer(String),
    PreEts4Style(Bool, default="0"), ConvertedFromPreEts4Data(Bool, default="0"),
    IPConfig(String, default="Tool"), AdditionalAddressesCount(Int32, default="0"),
    ReplacesVersions(String), IsSecureEnabled(Bool), TunnelingAddressIndices(String),
    MaxUserEntries(UInt16, default="0"), MaxTunnelingUserEntries(UInt16, default="0"),
    MaxSecurityIndividualAddressEntries(UInt16, default="0"),
    MaxSecurityGroupKeyTableEntries(UInt16, default="0"),
    MaxSecurityP2PKeyTableEntries(UInt16, default="0"),
    MaxSecurityProxyGroupKeyTableEntries(UInt16, default="0"),
    MaxSecurityProxyIndividualAddressTableEntries(UInt16, default="0")
AbsoluteSegment: Id(ApplProgId, sort), Address(UInt32), Size(UInt32), UserMemory(Bool, default="0")
RelativeSegment: Id(ApplProgId, sort), Offset(UInt32), Size(UInt32), LoadStateMachine(Byte)
Data: InnerTextBase64
ParameterType: Id(ApplProgId, sort), Name(String)
TypeNumber: SizeInBit(Byte), Type(String), minInclusive(Int64), maxInclusive(Int64), Increment(Int64, default="1")
TypeFloat: Encoding(String), minInclusive(Double), maxInclusive(Double), Increment(Double, default="1")
TypeRestriction: Base(String), SizeInBit(UInt32)
Enumeration: Id(ApplProgId, sort), Value(UInt32), BinaryValue(String)
TypeText: SizeInBit(UInt32)
Parameter: Id(ApplProgId, sort), ParameterType(ApplProgId), Value(DynamicValue), DefaultUnionParameter(Bool, default="0"), LegacyPatchAlways(Bool, default="0"), CustomerAdjustable(Bool, default="0"), Offset(UInt32, default="0"), BitOffset(Byte, default="0")
Memory: CodeSegment(ApplProgId, sort), Offset(UInt32), BitOffset(Byte)
ParameterRef: Id(ApplProgId, sort), RefId(ApplProgId), Value(DynamicValue), CustomerAdjustable(Bool, default="0")
ComObjectTable: CodeSegment(ApplProgId, sort), Offset(UInt32)
ComObject: Id(ApplProgId, sort), Number(UInt32), SecurityRequired(String, default="None"), MayRead(Bool, default="0"), ReadFlagLocked(Bool, default="0"), WriteFlagLocked(Bool, default="0"), TransmitFlagLocked(Bool, default="0"), UpdateFlagLocked(Bool, default="0"), ReadOnInitFlagLocked(Bool, default="0")
ComObjectRef: Id(ApplProgId, sort), RefId(ApplProgId), ObjectSize(String), MayRead(Bool, default="0"), ReadFlagLocked(Bool, default="0"), WriteFlagLocked(Bool, default="0"), TransmitFlagLocked(Bool, default="0"), UpdateFlagLocked(Bool, default="0"), ReadOnInitFlagLocked(Bool, default="0")
AddressTable: CodeSegment(ApplProgId), Offset(UInt32), MaxEntries(UInt32)
AssociationTable: CodeSegment(ApplProgId), Offset(UInt32), MaxEntries(UInt32)
... (many more LoadProcedure elements, Dynamic elements, etc.)
```

## NOT Registration Relevant

These ApplicationProgram attributes are NOT included in the hash:
- `Name`
- `DefaultLanguage`
- `MinEtsVersion`
- `NonRegRelevantDataVersion`
- `Hash` (the hash attribute itself)

## Verified Golden Vectors

| Input | MD5 Hash | Fingerprint |
|-------|----------|-------------|
| Minimal XML (1 AppProg, no params) | `24ab92da12d47b99de1c6728334c7b12` | `2412` |
| MDT Leakage Sensor | `dd03e07cbe5cc31594a44bea21f566af` | `DDAF` |

## Key Observations

1. The Hash attribute is NOT included in the hash computation
2. The Application ID is normalized to `0000` (fingerprint zeroed) before hashing
3. Default values are used for missing attributes (e.g. Bool defaults to "0")
4. Elements with `isSort: true` on their first attribute are sorted by that attribute's value
5. The hash covers the ENTIRE ApplicationProgram tree, not just the root element attributes
