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
   c. **Sort attributes alphabetically by their short name**
   d. Write each attribute as: `BinaryWriter.Write(shortName) + BinaryWriter.Write(typedValue)`
3. MD5 hash the entire byte stream → 16 bytes
4. Fingerprint = `(hash[0] << 8) | hash[15]` → 4 hex chars (e.g. "DDAF")
5. New Application ID = old ID with last 4 chars replaced by fingerprint

## Serialization Format (verified by byte dump)

Each attribute is serialized as a pair: **short name + typed value**, sorted alphabetically by short name.

### BinaryWriter encoding rules

| C# method | Encoding |
|-----------|----------|
| `Write(string)` | varint length prefix (7-bit encoded) + UTF-8 bytes |
| `Write(byte)` | 1 byte |
| `Write(bool)` | 1 byte: `0x00` = false, `0x01` = true |
| `Write(ushort)` | 2 bytes little-endian |
| `Write(int)` | 4 bytes little-endian |
| `Write(uint)` | 4 bytes little-endian |
| `Write(long)` | 8 bytes little-endian |
| `Write(double)` | 8 bytes IEEE 754 little-endian |

### Varint length encoding (7-bit)

```
length < 128:    1 byte  [length]
length < 16384:  2 bytes [length & 0x7F | 0x80, length >> 7]
```

### Null/missing values

- Missing string attributes: `BinaryWriter.Write("$<null>$")` → `08 24 3C 6E 75 6C 6C 3E 24`
- Missing bool attributes: `BinaryWriter.Write(false)` → `00` (uses default, typically "0")
- Missing UInt16 attributes: `BinaryWriter.Write((ushort)0)` → `00 00`
- Missing UInt32/Int32 attributes: `BinaryWriter.Write((uint)0)` → `00 00 00 00`

### ApplProgId normalization

Application Program IDs are normalized before hashing:
- The fingerprint suffix (last 4 hex chars after the last `-`) is **removed**
- Example: `M-0083_A-0001-01-0000` → written as `M-0083_A-0001-01` (16 chars)
- Example: `M-0083_A-014F-10-DDAF` → written as `M-0083_A-014F-10` (16 chars)

## Typed Attribute Serializers

| Type | Short name written as | Value written as |
|------|----------------------|-----------------|
| `StringAttributeInfo(name, short)` | `Write(short)` | `Write(value)` or `Write("$<null>$")` |
| `UInt16AttributeInfo(name, short)` | `Write(short)` | `Write((ushort)parsed)` |
| `UInt32AttributeInfo(name, short)` | `Write(short)` | `Write((uint)parsed)` |
| `ByteAttributeInfo(name, short)` | `Write(short)` | `Write((byte)parsed)` |
| `BoolAttributeInfo(name, short)` | `Write(short)` | `Write(parsed == "1" or "true")` |
| `Int32AttributeInfo(name, short)` | `Write(short)` | `Write((int)parsed)` |
| `Int64AttributeInfo(name, short)` | `Write(short)` | `Write((long)parsed)` |
| `DoubleAttributeInfo(name, short)` | `Write(short)` | `Write((double)parsed)` |
| `ApplProgIdAttributeInfo(name, short)` | `Write(short)` | `Write(normalized_id)` or `Write("$<null>$")` |
| `InnerTextBase64ElementInfo(char)` | (char as tag) | Base64-decoded bytes written directly |
| `InnerTextStringElementInfo(char)` | (char as tag) | `Write(innerText)` |

## Registration Relevant Elements (ApplicationProgram)

From `Knx.Ets.Xml.RegistrationRelevanceInformation.dll` (not obfuscated):

```
Manufacturer:
  RefId(String, "R", sort)

ApplicationProgram:
  Id(ApplProgId, "I", sort)
  ApplicationNumber(UInt16, "AN")
  ApplicationVersion(Byte, "AV")
  ProgramType(String, "PrT")
  MaskVersion(String, "MV")
  LoadProcedureStyle(String, "LPS")
  PeiType(Byte, "PT")
  DynamicTableManagement(Bool, "DTM")
  Linkable(Bool, "L")
  OriginalManufacturer(String, "OEM")
  PreEts4Style(Bool, "PES", default="0")
  ConvertedFromPreEts4Data(Bool, "CVETS", default="0")
  IPConfig(String, "IP", default="Tool")
  AdditionalAddressesCount(Int32, "AAC", default="0")
  ReplacesVersions(String, "RV")
  IsSecureEnabled(Bool, "ISE")
  TunnelingAddressIndices(String, "TAI")
  MaxUserEntries(UInt16, "MUE", default="0")
  MaxTunnelingUserEntries(UInt16, "MTUE", default="0")
  MaxSecurityIndividualAddressEntries(UInt16, "MSIAE", default="0")
  MaxSecurityGroupKeyTableEntries(UInt16, "MSGK", default="0")
  MaxSecurityP2PKeyTableEntries(UInt16, "MSP2", default="0")
  MaxSecurityProxyGroupKeyTableEntries(UInt16, "MSPGK", default="0")
  MaxSecurityProxyIndividualAddressTableEntries(UInt16, "MSPIA", default="0")

AbsoluteSegment:
  Id(ApplProgId, "I", sort), Address(UInt32, "A"), Size(UInt32, "S"), UserMemory(Bool, "UM", default="0")

RelativeSegment:
  Id(ApplProgId, "I", sort), Offset(UInt32, "O"), Size(UInt32, "S"), LoadStateMachine(Byte, "LSM")

Data: InnerTextBase64('D')
Mask: InnerTextBase64('M')

ParameterType:
  Id(ApplProgId, "I", sort), Name(String, "N")

TypeNumber:
  SizeInBit(Byte, "S"), Type(String, "T"), minInclusive(Int64, "min"), maxInclusive(Int64, "max"), Increment(Int64, "I", default="1")

TypeFloat:
  Encoding(String, "E"), minInclusive(Double, "min"), maxInclusive(Double, "max"), Increment(Double, "I", default="1")

TypeRestriction:
  Base(String, "B"), SizeInBit(UInt32, "S")

Enumeration:
  Id(ApplProgId, "I", sort), Value(UInt32, "V"), BinaryValue(String, "BV")

TypeText: SizeInBit(UInt32, "S")
TypeTime: SizeInBit(Byte, "S"), Unit(String, "U"), minInclusive(Int64, "min"), maxInclusive(Int64, "max")
TypeDate: Encoding(String, "E"), DisplayTheYear(Bool, "Y", default="1")
TypeIPAddress: AddressType(String, "AT"), Version(String, "V", default="IPv4")
TypeColor: Space(String, "S")
TypeRawData: MaxSize(UInt32, "M")

Parameter:
  Id(ApplProgId, "I", sort), ParameterType(ApplProgId, "PT"), Value(DynamicValue, "V"),
  DefaultUnionParameter(Bool, "DEF", default="0"), LegacyPatchAlways(Bool, "LPA", default="0"),
  CustomerAdjustable(Bool, "CA", default="0"), Offset(UInt32, "O", default="0"), BitOffset(Byte, "BO", default="0")

Memory:
  CodeSegment(ApplProgId, "C", sort), Offset(UInt32, "O"), BitOffset(Byte, "BO")

Property:
  ObjectIndex(Byte, "OI", sort), ObjectType(UInt16, "OT"), Occurrence(Byte, "OC", default="0"),
  PropertyId(Byte, "PID"), Offset(UInt32, "O"), BitOffset(Byte, "BO")

ParameterRef:
  Id(ApplProgId, "I", sort), RefId(ApplProgId, "R"), Value(DynamicValue, "V"), CustomerAdjustable(Bool, "CA", default="0")

ComObjectTable:
  CodeSegment(ApplProgId, "C", sort), Offset(UInt32, "O")

ComObject:
  Id(ApplProgId, "I", sort), Number(UInt32, "N"), SecurityRequired(String, "SR", default="None"),
  MayRead(Bool, "MR", default="0"), ReadFlagLocked(Bool, "RFL", default="0"),
  WriteFlagLocked(Bool, "WFL", default="0"), TransmitFlagLocked(Bool, "TFL", default="0"),
  UpdateFlagLocked(Bool, "UFL", default="0"), ReadOnInitFlagLocked(Bool, "ROIFL", default="0")

ComObjectRef:
  Id(ApplProgId, "I", sort), RefId(ApplProgId, "R"), ObjectSize(String, "S"),
  MayRead(Bool, "MR", default="0"), ReadFlagLocked(Bool, "RFL", default="0"),
  WriteFlagLocked(Bool, "WFL", default="0"), TransmitFlagLocked(Bool, "TFL", default="0"),
  UpdateFlagLocked(Bool, "UFL", default="0"), ReadOnInitFlagLocked(Bool, "ROIFL", default="0")

AddressTable:
  CodeSegment(ApplProgId, "C"), Offset(UInt32, "O"), MaxEntries(UInt32, "ATM")

AssociationTable:
  CodeSegment(ApplProgId, "C"), Offset(UInt32, "O"), MaxEntries(UInt32, "ASM")

Fixup: FunctionRef(String, "F"), CodeSegment(ApplProgId, "C")
Offset: InnerTextUInt32('O')

LoadProcedure: MergeId(Byte, "M")
LdCtrlUnload: (ordered) LsmIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), AppliesTo(String, "AT", default="auto")
LdCtrlLoad: (ordered) LsmIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), AppliesTo(String, "AT", default="auto")
LdCtrlMaxLength: (ordered) LsmIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), AppliesTo(String, "A", default="auto"), Size(UInt32, "S")
LdCtrlLoadCompleted: (ordered) LsmIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), AppliesTo(String, "AT", default="auto")
LdCtrlAbsSegment: (ordered) LsmIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), SegType(Byte, "ST"), Address(UInt16, "A"), Size(UInt32, "S"), Access(Byte, "AC"), MemType(Byte, "M"), SegFlags(Byte, "SF"), AppliesTo(String, "AT", default="auto")
LdCtrlRelSegment: (ordered) LsmIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), Size(UInt32, "S"), Mode(Byte, "M"), Fill(Byte, "F"), AppliesTo(String, "AT", default="auto")
LdCtrlTaskSegment: (ordered) LsmIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), Address(UInt16, "A"), AppliesTo(String, "AT", default="auto")
LdCtrlTaskPtr: (ordered) LsmIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), InitPtr(UInt16, "IP"), SavePtr(UInt16, "S"), SerialPtr(UInt16, "SP"), AppliesTo(String, "AT", default="auto")
LdCtrlTaskCtrl1: (ordered) LsmIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), Address(UInt16, "A"), Count(Byte, "C"), AppliesTo(String, "AT", default="auto")
LdCtrlTaskCtrl2: (ordered) LsmIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), Callback(UInt16, "C"), Address(UInt16, "A"), Seg0(UInt16, "S0"), Seg1(UInt16, "S1"), AppliesTo(String, "AT", default="auto")
LdCtrlWriteProp: (ordered) ObjIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), PropId(Byte, "P"), StartElement(UInt16, "S", default="1"), Count(UInt16, "C", default="1"), Verify(Bool, "V"), InlineData(String, "D"), AppliesTo(String, "AT", default="auto")
LdCtrlCompareProp: (ordered) ObjIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), PropId(Byte, "P"), StartElement(UInt16, "S", default="1"), Count(UInt16, "C", default="1"), InlineData(String, "D"), AppliesTo(String, "AT", default="auto"), AllowCachedValue(Bool, "ACV", default="0"), Mask(String, "M"), Range(String, "R"), Invert(Bool, "Inv", default="0"), RetryInterval(UInt16, "RI", default="0"), TimeOut(UInt16, "TO", default="0")
LdCtrlLoadImageProp: (ordered) ObjIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), PropId(Byte, "P"), Count(UInt16, "C", default="1"), StartElement(UInt16, "S", default="1"), AppliesTo(String, "AT", default="auto")
LdCtrlInvokeFunctionProp: (ordered) ObjIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), PropId(Byte, "P"), InlineData(String, "D"), AppliesTo(String, "AT", default="auto")
LdCtrlReadFunctionProp: (ordered) ObjIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), PropId(Byte, "P"), AppliesTo(String, "AT", default="auto")
LdCtrlWriteMem: (ordered) AddressSpace(String, "AS", default="Standard"), Address(UInt32, "A"), Size(UInt32, "S"), Verify(Bool, "V"), InlineData(String, "D"), AppliesTo(String, "AT", default="auto")
LdCtrlCompareMem: (ordered) AddressSpace(String, "AS", default="Standard"), Address(UInt32, "A"), Size(UInt32, "S"), InlineData(String, "D"), AppliesTo(String, "AT", default="auto"), AllowCachedValue(Bool, "ACV", default="0"), Mask(String, "M"), Range(String, "R"), Invert(Bool, "Inv", default="0"), RetryInterval(UInt16, "RI", default="0"), TimeOut(UInt16, "TO", default="0")
LdCtrlLoadImageMem: (ordered) AddressSpace(String, "AS", default="Standard"), Address(UInt32, "A"), Size(UInt32, "S"), AppliesTo(String, "AT", default="auto")
LdCtrlWriteRelMem: (ordered) ObjIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), Offset(UInt32, "O"), Size(UInt32, "S"), Verify(Bool, "V"), InlineData(String, "D"), AppliesTo(String, "AT", default="auto")
LdCtrlCompareRelMem: (ordered) ObjIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), Offset(UInt32, "O"), Size(UInt32, "S"), InlineData(String, "D"), AppliesTo(String, "AT", default="auto"), AllowCachedValue(Bool, "ACV", default="0"), Mask(String, "M"), Range(String, "R"), Invert(Bool, "Inv", default="0"), RetryInterval(UInt16, "RI", default="0"), TimeOut(UInt16, "TO", default="0")
LdCtrlLoadImageRelMem: (ordered) ObjIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), Offset(UInt32, "O"), Size(UInt32, "S"), AppliesTo(String, "AT", default="auto")
LdCtrlConnect: (ordered) AppliesTo(String, "AT", default="auto")
LdCtrlDisconnect: (ordered) AppliesTo(String, "AT", default="auto")
LdCtrlRestart: (ordered) AppliesTo(String, "AT", default="auto")
LdCtrlMasterReset: (ordered) EraseCode(Byte, "EC"), ChannelNumber(Byte, "CN"), AppliesTo(String, "AT", default="auto")
LdCtrlDelay: (ordered) MilliSeconds(UInt16, "M"), AppliesTo(String, "AT", default="auto")
LdCtrlSetControlVariable: (ordered) Name(String, "N"), Value(Bool, "V"), AppliesTo(String, "AT", default="auto")
LdCtrlMapError: (ordered) LdCtrlFilter(Byte, "L", default="0"), OriginalError(UInt32, "O"), MappedError(UInt32, "M"), AppliesTo(String, "AT", default="auto")
LdCtrlProgressText: (ordered) AppliesTo(String, "AT", default="auto")
LdCtrlDeclarePropDesc: (ordered) ObjIdx(Byte, "I"), ObjType(UInt16, "T"), Occurrence(Byte, "O", default="0"), PropId(Byte, "P"), PropType(String, "PT"), MaxElements(UInt16, "M"), ReadAccess(Byte, "R"), WriteAccess(Byte, "WA"), Writable(Bool, "W"), AppliesTo(String, "AT", default="auto")
LdCtrlClearLCFilterTable: (ordered) AppliesTo(String, "AT", default="auto"), UseFunctionProp(Bool, "U", default="0")
LdCtrlMerge: (ordered) MergeId(Byte, "M"), AppliesTo(String, "AT", default="auto")

OnError: Cause(String, "C", sort), Ignore(Bool, "I", default="0")
Script: InnerTextString('S')

SecurityRole: Id(ApplProgId, "I", sort), Mask(UInt16, "M")
BusInterface: Id(ApplProgId, "I", sort), AddressIndex(UInt16, "AI"), AccessType(String, "AT")
Allocator: Name(String, "N"), Start(Int64, "S"), maxInclusive(Int64, "MI")

Options:
  LineCoupler0912NewProgrammingStyle(Bool, "L", default="0"), NotLoadable(String, "NL"),
  CustomerAdjustableParameters(String, "CAP", default="0"), MaxRoutingApduLength(UInt32, "MPAL", default="0"),
  MasterResetOnCRCMismatch(Bool, "MR", default="0"), SupportsExtendedMemoryServices(Bool, "SEMS", default="0"),
  SupportsExtendedPropertyServices(Bool, "SEPS", default="0"), SupportsIpSystemBroadcast(Bool, "SISB", default="0"),
  LegacyPatchManufacturerIdInTaskSegment(Bool, "LPMTS", default="0")

Extension: ExtensionElement('B')
Argument: Name(String, "N"), Allocates(Int64, "A"), Alignment(UInt16, "Ali", default="1")

ParameterBlock: (ordered) Id(ApplProgId, "I"), ParamRefId(ApplProgId, "R")
ParameterSeparator: (ordered) Id(ApplProgId, "I")
ParameterRefRef: RefId(ApplProgId, "R"), AliasName(String, "AN", default=null)
Choose: (ordered) ParamRefId(ApplProgId, "R")
When: (ordered) test(String, "T"), default(Bool, "D", default="0")
BinaryDataRef: (ordered) RefId(ApplProgId, "R")
ComObjectRefRef: (ordered) RefId(ApplProgId, "R")
Assign: (ordered) TargetParamRefRef(ApplProgId, "T"), SourceParamRefRef(ApplProgId, "S"), Value(String, "V")
Rename: (ordered) Id(ApplProgId, "I"), RefId(ApplProgId, "R")
Channel: (ordered) Id(ApplProgId, "I"), Number(String, "N")
ChannelIndependentBlock: (ordered, no attributes)
Module: (ordered) RefId(ApplProgId, "R")
Repeat: (ordered) ParameterRefId(ApplProgId, "PRID"), Count(UInt32, "C", default="0")
NumericArg: RefId(ApplProgId, "R"), Value(Int64, "V"), AllocatorRefId(ApplProgId, "ARI"), BaseValue(ApplProgId, "BV")
TextArg: RefId(ApplProgId, "R")
```

## Element ordering

- Elements marked `(ordered)` preserve their order in the XML (order-relevant for hash)
- Elements NOT marked `(ordered)` are sorted by their sort attribute (first attribute with `sort` flag)
- Child elements are recursively hashed and their bytes appended to the parent's stream

## NOT Registration Relevant

These ApplicationProgram attributes are NOT included in the hash:
- `Name`
- `DefaultLanguage`
- `MinEtsVersion`
- `NonRegRelevantDataVersion`
- `Hash` (the hash attribute itself)

## Verified Golden Vectors

| Input | Pre-MD5 bytes | MD5 Hash | Fingerprint |
|-------|--------------|----------|-------------|
| Minimal XML (1 AppProg, no params) | `minimal_prebytes.bin` (307 bytes) | `24ab92da12d47b99de1c6728334c7b12` | `2412` |
| MDT Leakage Sensor | `leakage_prebytes.bin` (11297 bytes) | `dd03e07cbe5cc31594a44bea21f566af` | `DDAF` |

## Byte dump analysis (minimal_prebytes.bin, 307 bytes)

```
[  0] str(3): 'AAC'          → AdditionalAddressesCount short name
[  4] uint32: 0x00000000     → value 0 (default)
[  8] str(2): 'AN'           → ApplicationNumber short name
[011] ushort: 0x0001         → value 1
[013] str(2): 'AV'           → ApplicationVersion short name
[016] byte: 0x01             → value 1
[017] str(5): 'CVETS'        → ConvertedFromPreEts4Data short name
[023] bool: 0x00             → false (default)
[024] str(3): 'DTM'          → DynamicTableManagement short name
[028] bool: 0x00             → false
[029] str(1): 'I'            → Id short name
[031] str(16): 'M-0083_A-0001-01'  → normalized (no fingerprint)
[048] str(2): 'IP'           → IPConfig short name
[051] str(4): 'Tool'         → default value
[056] str(3): 'ISE'          → IsSecureEnabled short name
[060] str(8): '$<null>$'     → null (Bool without default → null string)
[069] str(1): 'L'            → Linkable short name
[071] bool: 0x00             → false
[072] str(3): 'LPS'          → LoadProcedureStyle short name
[076] str(16): 'ProductProcedure'
[093] str(2): 'MV'           → MaskVersion short name
[096] str(7): 'MV-0705'
[104] str(4): 'MSGK'         → MaxSecurityGroupKeyTableEntries
[109] ushort: 0x0000         → 0 (default)
... (more defaults)
[225] str(1): 'C'            → AddressTable.CodeSegment
[227] str(8): '$<null>$'     → null
[236] str(3): 'ATM'          → AddressTable.MaxEntries
[240] uint32: 0x000000FF     → 255
[244] str(1): 'O'            → AddressTable.Offset
[246] str(8): '$<null>$'     → null
... (ComObjectTable, AssociationTable follow)
```

## Key Observations

1. The Hash attribute is NOT included in the hash computation
2. The Application ID is normalized: fingerprint suffix removed (last 5 chars `-XXXX`)
3. Default values are used for missing attributes
4. Attributes are **sorted alphabetically by short name** within each element
5. Elements marked `(ordered)` preserve XML document order; others are sorted by their sort key
6. The hash covers the ENTIRE ApplicationProgram tree recursively
7. `$<null>$` is the null sentinel for missing string/ApplProgId attributes
8. Bool attributes without a value and without a default appear to use `$<null>$` (see ISE)
