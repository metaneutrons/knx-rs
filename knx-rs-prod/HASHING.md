# KNX ApplicationProgram Hashing Algorithm

## Overview

The ApplicationProgram hash is an MD5 hash over the "registration relevant" attributes
of all XML elements in the ApplicationProgram tree. The hash determines the fingerprint
(last 4 hex chars) in the Application Program ID.

## Algorithm

1. Navigate to `<ApplicationPrograms>` in the XML
2. Create a recursive `ChildElementHashGenerator` for its children
3. For each child element:
   a. Look up the element name in `RegistrationRelevantApplicationProgramElements`
   b. If found (registry element): serialize its attributes in **alphabetical order by XML attribute name**
   c. If not found (non-registry container): recursively process its children
4. All children at each level go into a **sorted collection** (SortedDictionary)
5. Sort key: explicit sort attribute value (e.g. Id), or `Base64(MD5(bytes))` for elements without sort key
6. Concatenate sorted children bytes → parent's byte stream
7. MD5 hash the entire byte stream → 16 bytes
8. Fingerprint = `(hash[0] << 8) | hash[15]` → 4 hex chars (e.g. "DDAF")

### Attribute ordering

Attributes within each registry element are sorted **alphabetically by their XML attribute name**
(not by short name, not by constructor order). This is because the C# `ElementInfo` constructor
calls `attributes.OrderBy(attr => attr.Name)` before storing them.

### Element ordering

- Elements marked `(ordered)` preserve their XML document order (use an integer counter as sort key)
- All other elements are sorted by their sort attribute value, or by `Base64(MD5(bytes))` if no sort attribute
- The sort uses .NET `SortedDictionary<object, byte[]>` with `Comparer<object>.Default`

### Sort order (.NET string.CompareTo / InvariantCulture)

The sort comparison matches .NET `string.CompareTo` which uses Unicode collation:

**Primary level** (case-insensitive):

```plain
SPACE  _  -  ,  ;  :  !  ?  .  '  "  (  )  [  ]  {  }  @  *  /  \  &  #  %  `  ^  +  <  =  >  |  ~  $  0-9  a=A  b=B  ...  z=Z
```

Key differences from ASCII order: `/` < `+` < `=`, `_` < `-`, symbols < digits < letters.

**Secondary level** (tiebreaker): lowercase before uppercase (`a` < `A`).

**Accented characters**: sorted near their base letter (ä≈a, ö≈o, ü≈u, etc.).

## Serialization Format (verified by byte dump)

Each attribute is serialized as a pair: **short name + typed value**, in alphabetical order by XML attribute name.

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

```plain
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

- Strip the 4-char hex fingerprint after the 2nd dash in the `_A-` part
- Preserve suffixes like `_MD-2`, `_O-1`, `-O000A`
- Example: `M-0083_A-0001-01-0000` → `M-0083_A-0001-01`
- Example: `M-0083_A-00B0-32-0DFC_MD-2_PC-1` → `M-0083_A-00B0-32_MD-2_PC-1`

### InnerText elements (Script, RLTransformation, LRTransformation, Data, Mask, Offset)

These elements serialize their text content rather than attributes:

- **InnerTextString** (Script `S`, RLTransformation `R`, LRTransformation `L`): tag byte + `BinaryWriter.Write(text)`
- **InnerTextBase64** (Data `D`, Mask `M`): tag byte + base64-decoded raw bytes
- **InnerTextUInt32** (Offset `O`): despite the name, writes as `BinaryWriter.Write(string)` (not as uint32)

### InnerText overshoot (C# bug, faithfully replicated)

When an InnerText element is **empty** (e.g. `<Script />`), the C# `GetHashBytesOfCurrentNode`
reads forward through the XML stream looking for any Text node — crossing element boundaries.
This "overshoot" can read text from a completely different element deep in the tree.

The overshoot causes **cascading stops**: each parent generator stops at the first EndElement
it encounters (which belongs to the overshooting element's context), causing elements to
"fall through" to higher levels of the tree.

### Text content handling

- **Line endings**: `\r\n` → `\n` and lone `\r` → `\n` (matching C# XmlReader normalization)
- **XML entities**: `&lt;` → `<`, `&gt;` → `>`, `&amp;` → `&`, `&apos;` → `'`, `&quot;` → `"`
- **CDATA sections**: `<![CDATA[...]]>` content is treated as text (no entity decoding needed)
- **`IsNullOrEmpty` handling**: empty XML attribute values (`""`) are treated as missing (use default or null sentinel)
- Text content may span multiple quick_xml events (Text + GeneralRef + Text) due to entity splitting

### ParameterRefRef parent-conditional ordering

`ParameterRefRef` is normally ordered (preserves document order). However, when the parent
element is `LParameters` or `RParameters`, ordering is disabled and elements are sorted by
`Base64(MD5(bytes))` instead. This matches the C# `ParameterRefRefElementInfo.OrderIsRelevant`
method which returns `false` for these specific parents.

### ParameterCalculation and ParameterValidation elements

`ParameterCalculation` and `ParameterValidation` are registry elements (not just containers).

`ParameterCalculation` attributes: `Id` (sort key), `Language`, `LRTransformationFunc` (default "1"),
`LRTransformationParameters` (default "1"), `RLTransformationFunc` (default "1"),
`RLTransformationParameters` (default "1"). Contains RLTransformation, LRTransformation,
LParameters, RParameters children.

`ParameterValidation` attributes: `Id` (sort key), `ValidationFunc`, `ValidationParameters` (default "0").
Contains ParameterRefRef children with AliasName bindings.

### Lowercase element variants

Both `choose`/`Choose` and `when`/`When` are registered (lowercase variants appear in Dynamic sections).

### LoadProcedure

`LoadProcedure` is NOT ordered — its children are sorted by MD5-Base64 key, not document order.

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

All 89 registration-relevant element types from the ETS registry (plus lowercase
`choose`/`when` variants).

```plain
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
LdCtrlClearCachedObjectTypes: (ordered) AppliesTo(String, "A", default="auto")
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
ParameterRefRef: RefId(ApplProgId, "R"), AliasName(String, "AN", default=null)  [ordered except when parent is LParameters/RParameters]
ParameterCalculation:
  Id(ApplProgId, "I", sort), Language(String, "L"),
  LRTransformationFunc(String, "LRF", default="1"), LRTransformationParameters(String, "LRP", default="1"),
  RLTransformationFunc(String, "RLF", default="1"), RLTransformationParameters(String, "RLP", default="1")
ParameterValidation:
  Id(ApplProgId, "I", sort), ValidationFunc(String, "VF"), ValidationParameters(String, "VP", default="0")
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

## Non-registration-relevant attributes

These ApplicationProgram attributes are NOT included in the hash:
`Name`, `DefaultLanguage`, `MinEtsVersion`, `NonRegRelevantDataVersion`, `Hash` (the hash itself).

## Verified Golden Vectors

28 files across 5 manufacturers/projects, all verified byte-exact against the C# reference tool.

| Manufacturer | File | Bytes | Notable features |
|---|---|---|---|
| — | Minimal XML | 307 | Baseline: attrs, defaults, null sentinel |
| MDT | Leakage Sensor | 11,297 | ComObjectRef, ParameterType |
| MDT | AKK Switch Actuator 24ch | 1,024,714 | Large device |
| MDT | BE Binary Input 32ch | 62,082 | Script overshoot, `\r\n`, `&lt;` entities, TypeFloat doubles, ParameterCalculation |
| MDT | JAL Shutter Actuator 8ch | 1,357,252 | CDATA, non-ASCII sort keys (ä, ö), ParameterRefRef parent ordering |
| Gira | Tastsensor (small) | — | Different manufacturer |
| Gira | Busankoppler (medium) | — | |
| Gira | Dimmaktor 4fach (large) | — | |
| ABB | SBRU6/1 | 2,534,475 | Rename (ordered), different XML patterns |
| ABB | SBCU6/1 | 2,694,266 | |
| ABB | SBCU10/1 | 3,584,313 | |
| ABB | SBSU6/1 | 2,534,475 | |
| ABB | SBSU10/1 | 3,424,020 | |
| Siemens | LK V01.24 | 9,689 | Small Siemens device |
| Siemens | UP 204/2 | 4,361,496 | Large Siemens device |
| Siemens | RDG200KN | 290,021 | |
| Siemens | RDG260KN | 308,203 | |
| Siemens | QAA2150KT | 8,173 | |
| Siemens | QFA2250KT | 9,593 | |
| Siemens | QPA2350KT | 13,758 | |
| Siemens | OCT200 KNR | 1,622 | |
| Siemens | OCT110 BR | 40,208 | ParameterValidation |
| OpenKNX | SmartHomeBridge v2.1.1 | 18,288,216 | Largest test (62MB XML), open-source |
| OpenKNX | LogicModule v3.5.2 | 16,328,070 | Open-source, 51MB XML |

## Byte dump analysis (minimal_prebytes.bin, 307 bytes)

```plain
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

1. The `Hash` attribute itself is NOT included in the hash computation
2. Application IDs are normalized: fingerprint stripped from `_A-XXXX-YY-FFFF` part, suffixes preserved
3. Default values are used for missing attributes; `$<null>$` is the null sentinel
4. Attributes are sorted **alphabetically by XML attribute name** (not short name)
5. Elements marked `(ordered)` preserve XML document order; others sorted by sort key or MD5-Base64
6. The hash covers the ENTIRE ApplicationProgram tree recursively
7. `IsNullOrEmpty` semantics: empty string `""` treated as missing
8. Sort order matches .NET InvariantCulture: `/` < `+` < `=` < digits < letters (not ASCII order)
9. Empty InnerText elements cause an overshoot scan that crosses element boundaries (C# bug)
10. `ParameterRefRef` ordering is parent-conditional (unordered in LParameters/RParameters)
11. Line endings normalized: `\r\n` → `\n`
12. XML entities decoded in text content; CDATA sections preserved as-is
13. `LoadProcedure` children are NOT ordered (sorted by MD5-Base64)
