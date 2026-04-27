// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! `ApplicationProgram` hashing for KNX ETS product databases.
//!
//! Computes the registration-relevant MD5 hash over the `ApplicationProgram`
//! XML subtree, following the ETS `ApplicationProgram` hashing algorithm.

use std::collections::HashMap;
use std::fmt::Write as _;

use base64::Engine as _;
use md5::{Digest, Md5};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::KnxprodError;

const NULL_SENTINEL: &str = "$<null>$";

// -- BinaryWriter helpers --

fn write_varint(buf: &mut Vec<u8>, mut value: usize) {
    loop {
        #[allow(clippy::cast_possible_truncation)]
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len());
    buf.extend_from_slice(s.as_bytes());
}

fn write_bool(buf: &mut Vec<u8>, v: bool) {
    buf.push(u8::from(v));
}
fn write_byte(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}
fn write_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn write_i64(buf: &mut Vec<u8>, v: i64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

// -- Attribute types --

#[derive(Debug, Clone, Copy)]
enum AttrType {
    String,
    ApplProgId,
    Bool,
    UInt16,
    UInt32,
    Int32,
    Int64,
    Byte,
    Double,
}

#[derive(Debug, Clone, Copy)]
struct AttrInfo {
    xml_name: &'static str,
    short: &'static str,
    attr_type: AttrType,
    default: Option<&'static str>,
}

/// Element kind: normal attributes, or inner-text-base64 (Data/Mask).
#[derive(Debug, Clone, Copy)]
enum ElementKind {
    /// Normal element with typed attributes. Optional sort attribute `xml_name`.
    Attrs(&'static [AttrInfo], Option<&'static str>),
    /// `InnerTextBase64` — tag char written as raw byte, then base64-decoded
    /// content written directly (no length prefix).
    InnerTextBase64(u8),
    /// `InnerTextUInt32` — tag char as raw byte, then inner text parsed as u32 LE.
    InnerTextUInt32(u8),
    /// `InnerTextString` — tag char as raw byte, then inner text as `BinaryWriter` string.
    InnerTextString(u8),
}

#[derive(Debug, Clone, Copy)]
struct ElementInfo {
    kind: ElementKind,
    /// Ordered elements preserve XML document order among siblings.
    ordered: bool,
}

fn write_attr(buf: &mut Vec<u8>, info: &AttrInfo, raw: Option<&str>) {
    write_string(buf, info.short);
    // C# uses string.IsNullOrEmpty: empty strings are treated as missing.
    let raw = raw.filter(|s| !s.is_empty());
    let effective = raw.or(info.default);
    match (&info.attr_type, effective) {
        (_, None) => write_string(buf, NULL_SENTINEL),
        (AttrType::String, Some(v)) => write_string(buf, v),
        (AttrType::ApplProgId, Some(v)) => write_string(buf, &normalize_appl_prog_id(v)),
        (AttrType::Bool, Some(v)) => {
            write_bool(buf, v == "1" || v.eq_ignore_ascii_case("true"));
        }
        (AttrType::UInt16, Some(v)) => write_u16(buf, v.parse().unwrap_or(0)),
        (AttrType::UInt32, Some(v)) => write_u32(buf, v.parse().unwrap_or(0)),
        (AttrType::Int32, Some(v)) => {
            let n: i32 = v.parse().unwrap_or(0);
            #[allow(clippy::cast_sign_loss)]
            write_u32(buf, n as u32);
        }
        (AttrType::Int64, Some(v)) => write_i64(buf, v.parse().unwrap_or(0)),
        (AttrType::Byte, Some(v)) => write_byte(buf, v.parse().unwrap_or(0)),
        (AttrType::Double, Some(v)) => {
            let d: f64 = v.parse().unwrap_or(0.0);
            buf.extend_from_slice(&d.to_le_bytes());
        }
    }
}

fn normalize_appl_prog_id(id: &str) -> String {
    // Application Program IDs: M-XXXX_A-YYYY-ZZ-FFFF[_suffix | -suffix]
    // FFFF is the 4-char hex fingerprint (3rd dash-segment after "_A-").
    // Remove "-FFFF" but keep everything after it.
    if let Some(a_pos) = id.find("_A-") {
        let after_a = &id[a_pos + 3..];
        // Find the 2nd dash in after_a → start of fingerprint
        let mut dashes = 0;
        for (i, c) in after_a.char_indices() {
            if c == '-' {
                dashes += 1;
                if dashes == 2 {
                    // i points to the '-' before the fingerprint
                    // The fingerprint is 4 hex chars after this dash
                    let fp_start = a_pos + 3 + i; // position of '-' in original
                    let fp_end = fp_start + 5; // '-' + 4 hex chars
                    if fp_end <= id.len() {
                        let mut result = String::with_capacity(id.len());
                        result.push_str(&id[..fp_start]);
                        result.push_str(&id[fp_end..]);
                        return result;
                    }
                    break;
                }
            }
        }
    }
    id.to_string()
}

// Shorthand constructors for AttrInfo (keeps the registry compact).
const fn a(
    xml_name: &'static str,
    short: &'static str,
    attr_type: AttrType,
    default: Option<&'static str>,
) -> AttrInfo {
    AttrInfo {
        xml_name,
        short,
        attr_type,
        default,
    }
}
const fn el(attrs: &'static [AttrInfo]) -> ElementInfo {
    ElementInfo {
        kind: ElementKind::Attrs(attrs, None),
        ordered: false,
    }
}
const fn els(attrs: &'static [AttrInfo], sort_attr: &'static str) -> ElementInfo {
    ElementInfo {
        kind: ElementKind::Attrs(attrs, Some(sort_attr)),
        ordered: false,
    }
}
/// Ordered element — preserves XML document order among siblings.
const fn elo(attrs: &'static [AttrInfo]) -> ElementInfo {
    ElementInfo {
        kind: ElementKind::Attrs(attrs, None),
        ordered: true,
    }
}
const fn strt(tag: u8) -> ElementInfo {
    ElementInfo {
        kind: ElementKind::InnerTextString(tag),
        ordered: false,
    }
}
const fn u32t(tag: u8) -> ElementInfo {
    ElementInfo {
        kind: ElementKind::InnerTextUInt32(tag),
        ordered: false,
    }
}
const fn b64(tag: u8) -> ElementInfo {
    ElementInfo {
        kind: ElementKind::InnerTextBase64(tag),
        ordered: false,
    }
}

// ---------------------------------------------------------------------------
// Element registry — order matches C# Dictionary iteration order.
// Attributes within each element sorted by xml_name (case-insensitive).
// ---------------------------------------------------------------------------

use AttrType::{
    ApplProgId as AP, Bool as Bo, Byte as By, Double as Dbl, Int32 as I32, Int64 as I64,
    String as St, UInt16 as U16, UInt32 as U32,
};

#[rustfmt::skip]
const REGISTRY_ORDER: &[(&str, ElementInfo)] = &[
    // 0
    ("ApplicationProgram", els(&[
        a("AdditionalAddressesCount","AAC",I32,Some("0")), a("ApplicationNumber","AN",U16,None),
        a("ApplicationVersion","AV",By,None), a("ConvertedFromPreEts4Data","CVETS",Bo,Some("0")),
        a("DynamicTableManagement","DTM",Bo,None), a("Id","I",AP,None),
        a("IPConfig","IP",St,Some("Tool")), a("IsSecureEnabled","ISE",Bo,None),
        a("Linkable","L",Bo,None), a("LoadProcedureStyle","LPS",St,None),
        a("MaskVersion","MV",St,None), a("MaxSecurityGroupKeyTableEntries","MSGK",U16,Some("0")),
        a("MaxSecurityIndividualAddressEntries","MSIAE",U16,Some("0")),
        a("MaxSecurityP2PKeyTableEntries","MSP2",U16,Some("0")),
        a("MaxSecurityProxyGroupKeyTableEntries","MSPGK",U16,Some("0")),
        a("MaxSecurityProxyIndividualAddressTableEntries","MSPIA",U16,Some("0")),
        a("MaxTunnelingUserEntries","MTUE",U16,Some("0")), a("MaxUserEntries","MUE",U16,Some("0")),
        a("OriginalManufacturer","OEM",St,None), a("PeiType","PT",By,None),
        a("PreEts4Style","PES",Bo,Some("0")), a("ProgramType","PrT",St,None),
        a("ReplacesVersions","RV",St,None), a("TunnelingAddressIndices","TAI",St,None),
    ], "Id")),
    // 1
    ("ComObjectRef", els(&[
        a("Id","I",AP,None), a("MayRead","MR",Bo,Some("0")), a("ObjectSize","S",St,None),
        a("ReadFlagLocked","RFL",Bo,Some("0")), a("ReadOnInitFlagLocked","ROIFL",Bo,Some("0")),
        a("RefId","R",AP,None), a("TransmitFlagLocked","TFL",Bo,Some("0")),
        a("UpdateFlagLocked","UFL",Bo,Some("0")), a("WriteFlagLocked","WFL",Bo,Some("0")),
    ], "Id")),
    // 2
    ("AbsoluteSegment", els(&[
        a("Address","A",U32,None), a("Id","I",AP,None), a("Size","S",U32,None),
        a("UserMemory","UM",Bo,Some("0")),
    ], "Id")),
    // 3 + 4
    ("Data", b64(b'D')),
    ("Mask", b64(b'M')),
    // 5
    ("AddressTable", el(&[
        a("CodeSegment","C",AP,None), a("MaxEntries","ATM",U32,None), a("Offset","O",U32,None),
    ])),
    // 6
    ("LoadProcedure", el(&[a("MergeId","M",By,None)])),
    // 7
    ("LdCtrlConnect", elo(&[a("AppliesTo","AT",St,Some("auto"))])),
    // 8
    ("LdCtrlCompareProp", elo(&[
        a("AllowCachedValue","ACV",Bo,Some("0")), a("AppliesTo","AT",St,Some("auto")),
        a("Count","C",U16,Some("1")), a("InlineData","D",St,None),
        a("Invert","Inv",Bo,Some("0")), a("Mask","M",St,None),
        a("ObjIdx","I",By,None), a("ObjType","T",U16,None),
        a("Occurrence","O",By,Some("0")), a("PropId","P",By,None),
        a("Range","R",St,None), a("RetryInterval","RI",U16,Some("0")),
        a("StartElement","S",U16,Some("1")), a("TimeOut","TO",U16,Some("0")),
    ])),
    // 9
    ("LdCtrlUnload", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("LsmIdx","I",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")),
    ])),
    // 10
    ("LdCtrlAbsSegment", elo(&[
        a("Access","AC",By,None), a("Address","A",U16,None),
        a("AppliesTo","AT",St,Some("auto")), a("LsmIdx","I",By,None),
        a("MemType","M",By,None), a("ObjType","T",U16,None),
        a("Occurrence","O",By,Some("0")), a("SegFlags","SF",By,None),
        a("SegType","ST",By,None), a("Size","S",U32,None),
    ])),
    // 11
    ("LdCtrlTaskSegment", elo(&[
        a("Address","A",U16,None), a("AppliesTo","AT",St,Some("auto")),
        a("LsmIdx","I",By,None), a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")),
    ])),
    // 12
    ("LdCtrlLoad", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("LsmIdx","I",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")),
    ])),
    // 13
    ("LdCtrlLoadCompleted", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("LsmIdx","I",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")),
    ])),
    // 14
    ("LdCtrlRestart", elo(&[a("AppliesTo","AT",St,Some("auto"))])),
    // 15
    ("LdCtrlDisconnect", elo(&[a("AppliesTo","AT",St,Some("auto"))])),
    // 16
    ("Options", el(&[
        a("CustomerAdjustableParameters","CAP",St,Some("0")),
        a("LegacyPatchManufacturerIdInTaskSegment","LPMTS",Bo,Some("0")),
        a("LineCoupler0912NewProgrammingStyle","L",Bo,Some("0")),
        a("MasterResetOnCRCMismatch","MR",Bo,Some("0")),
        a("MaxRoutingApduLength","MPAL",U32,Some("0")),
        a("NotLoadable","NL",St,None),
        a("SupportsExtendedMemoryServices","SEMS",Bo,Some("0")),
        a("SupportsExtendedPropertyServices","SEPS",Bo,Some("0")),
        a("SupportsIpSystemBroadcast","SISB",Bo,Some("0")),
    ])),
    // 17
    ("ComObjectTable", els(&[a("CodeSegment","C",AP,None), a("Offset","O",U32,None)], "CodeSegment")),
    // 18
    ("ComObject", els(&[
        a("Id","I",AP,None), a("MayRead","MR",Bo,Some("0")), a("Number","N",U32,None),
        a("ReadFlagLocked","RFL",Bo,Some("0")), a("ReadOnInitFlagLocked","ROIFL",Bo,Some("0")),
        a("SecurityRequired","SR",St,Some("None")),
        a("TransmitFlagLocked","TFL",Bo,Some("0")),
        a("UpdateFlagLocked","UFL",Bo,Some("0")), a("WriteFlagLocked","WFL",Bo,Some("0")),
    ], "Id")),
    // 19
    ("ParameterType", els(&[a("Id","I",AP,None), a("Name","N",St,None)], "Id")),
    // 20
    ("TypeNumber", el(&[
        a("Increment","I",I64,Some("1")), a("maxInclusive","max",I64,None),
        a("minInclusive","min",I64,None), a("SizeInBit","S",By,None), a("Type","T",St,None),
    ])),
    // 21
    ("TypeRestriction", el(&[a("Base","B",St,None), a("SizeInBit","S",U32,None)])),
    // 22
    ("Enumeration", els(&[
        a("BinaryValue","BV",St,None), a("Id","I",AP,None), a("Value","V",U32,None),
    ], "Id")),
    // 23
    ("ParameterRef", els(&[
        a("CustomerAdjustable","CA",Bo,Some("0")), a("Id","I",AP,None),
        a("RefId","R",AP,None), a("Value","V",St,None),
    ], "Id")),
    // 24
    ("AssociationTable", el(&[
        a("CodeSegment","C",AP,None), a("MaxEntries","ASM",U32,None), a("Offset","O",U32,None),
    ])),
    // 25
    ("Parameter", els(&[
        a("BitOffset","BO",By,Some("0")), a("CustomerAdjustable","CA",Bo,Some("0")),
        a("DefaultUnionParameter","DEF",Bo,Some("0")), a("Id","I",AP,None),
        a("LegacyPatchAlways","LPA",Bo,Some("0")), a("Offset","O",U32,Some("0")),
        a("ParameterType","PT",AP,None), a("Value","V",St,None),
    ], "Id")),
    // 26
    ("Memory", els(&[
        a("BitOffset","BO",By,None), a("CodeSegment","C",AP,None), a("Offset","O",U32,None),
    ], "CodeSegment")),
    // 27
    ("ParameterBlock", elo(&[a("Id","I",AP,None), a("ParamRefId","R",AP,None)])),
    ("ParameterCalculation", els(&[
        a("Id","I",AP,None), a("Language","L",St,None),
        a("LRTransformationFunc","LRF",St,Some("1")), a("LRTransformationParameters","LRP",St,Some("1")),
        a("RLTransformationFunc","RLF",St,Some("1")), a("RLTransformationParameters","RLP",St,Some("1")),
    ], "Id")),
    // 28
    ("ComObjectRefRef", elo(&[a("RefId","R",AP,None)])),
    // 29
    ("ParameterRefRef", elo(&[a("AliasName","AN",St,None), a("RefId","R",AP,None)])),
    ("ParameterValidation", els(&[
        a("Id","I",AP,None), a("ValidationFunc","VF",St,None),
        a("ValidationParameters","VP",St,Some("0")),
    ], "Id")),
    // 30
    ("When", elo(&[a("default","D",Bo,Some("0")), a("test","T",St,None)])),
    ("when", elo(&[a("default","D",Bo,Some("0")), a("test","T",St,None)])),
    // 31
    ("ParameterSeparator", elo(&[a("Id","I",AP,None)])),
    // 32
    ("Choose", elo(&[a("ParamRefId","R",AP,None)])),
    ("choose", elo(&[a("ParamRefId","R",AP,None)])),
    // Remaining elements from spec (not in leakage vector but needed for completeness)
    ("RelativeSegment", els(&[
        a("Id","I",AP,None), a("LoadStateMachine","LSM",By,None),
        a("Offset","O",U32,None), a("Size","S",U32,None),
    ], "Id")),
    ("TypeFloat", el(&[
        a("Encoding","E",St,None), a("Increment","I",Dbl,Some("1")),
        a("maxInclusive","max",Dbl,None), a("minInclusive","min",Dbl,None),
    ])),
    ("TypeText", el(&[a("SizeInBit","S",U32,None)])),
    ("TypeTime", el(&[
        a("maxInclusive","max",I64,None), a("minInclusive","min",I64,None),
        a("SizeInBit","S",By,None), a("Unit","U",St,None),
    ])),
    ("TypeDate", el(&[
        a("DisplayTheYear","Y",Bo,Some("1")), a("Encoding","E",St,None),
    ])),
    ("TypeIPAddress", el(&[
        a("AddressType","AT",St,None), a("Version","V",St,Some("IPv4")),
    ])),
    ("TypeColor", el(&[a("Space","S",St,None)])),
    ("TypeRawData", el(&[a("MaxSize","M",U32,None)])),
    ("Property", el(&[
        a("BitOffset","BO",By,None), a("ObjectIndex","OI",By,None),
        a("ObjectType","OT",U16,None), a("Occurrence","OC",By,Some("0")),
        a("Offset","O",U32,None), a("PropertyId","PID",By,None),
    ])),
    ("Offset", u32t(b'O')),
    ("Fixup", el(&[a("CodeSegment","C",AP,None), a("FunctionRef","F",St,None)])),
    ("OnError", els(&[a("Cause","C",St,None), a("Ignore","I",Bo,Some("0"))], "Cause")),
    ("SecurityRole", els(&[a("Id","I",AP,None), a("Mask","M",U16,None)], "Id")),
    ("BusInterface", els(&[
        a("AccessType","AT",St,None), a("AddressIndex","AI",U16,None), a("Id","I",AP,None),
    ], "Id")),
    ("Allocator", el(&[
        a("maxInclusive","MI",I64,None), a("Name","N",St,None), a("Start","S",I64,None),
    ])),
    ("Argument", el(&[
        a("Alignment","Ali",U16,Some("1")), a("Allocates","A",I64,None), a("Name","N",St,None),
    ])),
    ("Extension", el(&[/* ExtensionElement — not yet implemented */])),
    ("ChannelIndependentBlock", elo(&[])),
    ("Channel", elo(&[a("Id","I",AP,None), a("Number","N",St,None)])),
    ("Rename", elo(&[a("Id","I",AP,None), a("RefId","R",AP,None)])),
    ("Assign", elo(&[
        a("SourceParamRefRef","S",AP,None), a("TargetParamRefRef","T",AP,None),
        a("Value","V",St,None),
    ])),
    ("BinaryDataRef", elo(&[a("RefId","R",AP,None)])),
    ("Module", elo(&[a("RefId","R",AP,None)])),
    ("Repeat", elo(&[a("Count","C",U32,Some("0")), a("ParameterRefId","PRID",AP,None)])),
    ("NumericArg", el(&[
        a("AllocatorRefId","ARI",AP,None), a("BaseValue","BV",AP,None),
        a("RefId","R",AP,None), a("Value","V",I64,None),
    ])),
    ("TextArg", el(&[a("RefId","R",AP,None)])),
    // LdCtrl variants not in leakage vector
    ("LdCtrlMaxLength", elo(&[
        a("AppliesTo","A",St,Some("auto")), a("LsmIdx","I",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")), a("Size","S",U32,None),
    ])),
    ("LdCtrlRelSegment", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("Fill","F",By,None),
        a("LsmIdx","I",By,None), a("Mode","M",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")), a("Size","S",U32,None),
    ])),
    ("LdCtrlTaskPtr", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("InitPtr","IP",U16,None),
        a("LsmIdx","I",By,None), a("ObjType","T",U16,None),
        a("Occurrence","O",By,Some("0")), a("SavePtr","S",U16,None),
        a("SerialPtr","SP",U16,None),
    ])),
    ("LdCtrlTaskCtrl1", elo(&[
        a("Address","A",U16,None), a("AppliesTo","AT",St,Some("auto")),
        a("Count","C",By,None), a("LsmIdx","I",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")),
    ])),
    ("LdCtrlTaskCtrl2", elo(&[
        a("Address","A",U16,None), a("AppliesTo","AT",St,Some("auto")),
        a("Callback","C",U16,None), a("LsmIdx","I",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")),
        a("Seg0","S0",U16,None), a("Seg1","S1",U16,None),
    ])),
    ("LdCtrlWriteProp", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("Count","C",U16,Some("1")),
        a("InlineData","D",St,None), a("ObjIdx","I",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")),
        a("PropId","P",By,None), a("StartElement","S",U16,Some("1")),
        a("Verify","V",Bo,None),
    ])),
    ("LdCtrlLoadImageProp", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("Count","C",U16,Some("1")),
        a("ObjIdx","I",By,None), a("ObjType","T",U16,None),
        a("Occurrence","O",By,Some("0")), a("PropId","P",By,None),
        a("StartElement","S",U16,Some("1")),
    ])),
    ("LdCtrlInvokeFunctionProp", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("InlineData","D",St,None),
        a("ObjIdx","I",By,None), a("ObjType","T",U16,None),
        a("Occurrence","O",By,Some("0")), a("PropId","P",By,None),
    ])),
    ("LdCtrlReadFunctionProp", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("ObjIdx","I",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")),
        a("PropId","P",By,None),
    ])),
    ("LdCtrlWriteMem", elo(&[
        a("Address","A",U32,None), a("AddressSpace","AS",St,Some("Standard")),
        a("AppliesTo","AT",St,Some("auto")), a("InlineData","D",St,None),
        a("Size","S",U32,None), a("Verify","V",Bo,None),
    ])),
    ("LdCtrlCompareMem", elo(&[
        a("Address","A",U32,None), a("AddressSpace","AS",St,Some("Standard")),
        a("AllowCachedValue","ACV",Bo,Some("0")), a("AppliesTo","AT",St,Some("auto")),
        a("InlineData","D",St,None), a("Invert","Inv",Bo,Some("0")),
        a("Mask","M",St,None), a("Range","R",St,None),
        a("RetryInterval","RI",U16,Some("0")), a("Size","S",U32,None),
        a("TimeOut","TO",U16,Some("0")),
    ])),
    ("LdCtrlLoadImageMem", elo(&[
        a("Address","A",U32,None), a("AddressSpace","AS",St,Some("Standard")),
        a("AppliesTo","AT",St,Some("auto")), a("Size","S",U32,None),
    ])),
    ("LdCtrlWriteRelMem", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("InlineData","D",St,None),
        a("ObjIdx","I",By,None), a("ObjType","T",U16,None),
        a("Occurrence","O",By,Some("0")), a("Offset","O",U32,None),
        a("Size","S",U32,None), a("Verify","V",Bo,None),
    ])),
    ("LdCtrlCompareRelMem", elo(&[
        a("AllowCachedValue","ACV",Bo,Some("0")), a("AppliesTo","AT",St,Some("auto")),
        a("InlineData","D",St,None), a("Invert","Inv",Bo,Some("0")),
        a("Mask","M",St,None), a("ObjIdx","I",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")),
        a("Offset","O",U32,None), a("Range","R",St,None),
        a("RetryInterval","RI",U16,Some("0")), a("Size","S",U32,None),
        a("TimeOut","TO",U16,Some("0")),
    ])),
    ("LdCtrlLoadImageRelMem", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("ObjIdx","I",By,None),
        a("ObjType","T",U16,None), a("Occurrence","O",By,Some("0")),
        a("Offset","O",U32,None), a("Size","S",U32,None),
    ])),
    ("LdCtrlMasterReset", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("ChannelNumber","CN",By,None),
        a("EraseCode","EC",By,None),
    ])),
    ("LdCtrlDelay", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("MilliSeconds","M",U16,None),
    ])),
    ("LdCtrlSetControlVariable", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("Name","N",St,None), a("Value","V",Bo,None),
    ])),
    ("LdCtrlMapError", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("LdCtrlFilter","L",By,Some("0")),
        a("MappedError","M",U32,None), a("OriginalError","O",U32,None),
    ])),
    ("LdCtrlProgressText", elo(&[a("AppliesTo","AT",St,Some("auto"))])),
    ("LdCtrlDeclarePropDesc", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("LsmIdx","I",By,None),
        a("MaxElements","M",U16,None), a("ObjType","T",U16,None),
        a("Occurrence","O",By,Some("0")), a("PropId","P",By,None),
        a("PropType","PT",St,None), a("ReadAccess","R",By,None),
        a("Writable","W",Bo,None), a("WriteAccess","WA",By,None),
    ])),
    ("LdCtrlClearLCFilterTable", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("UseFunctionProp","U",Bo,Some("0")),
    ])),
    ("LdCtrlClearCachedObjectTypes", elo(&[a("AppliesTo","A",St,Some("auto"))])),
    ("LdCtrlMerge", elo(&[
        a("AppliesTo","AT",St,Some("auto")), a("MergeId","M",By,None),
    ])),
    ("RLTransformation", strt(b'R')),
    ("LRTransformation", strt(b'L')),
    ("Script", strt(b'S')),
    ("Manufacturer", el(&[a("RefId","R",St,None)])),
];

fn build_registry() -> HashMap<&'static str, (usize, &'static ElementInfo)> {
    REGISTRY_ORDER
        .iter()
        .enumerate()
        .map(|(i, (name, info))| (*name, (i, info)))
        .collect()
}

// ---------------------------------------------------------------------------
// XML traversal — forward-only reader mirroring C# ChildElementHashGenerator
// ---------------------------------------------------------------------------

fn local_name(tag: &[u8]) -> &[u8] {
    tag.iter()
        .position(|&b| b == b':')
        .map_or(tag, |pos| &tag[pos + 1..])
}

/// Result of processing one XML element (mirrors C# `ChildElementHashGenerator`).
struct GenResult {
    bytes: Vec<u8>,
    order_key: Option<String>,
    order_is_relevant: bool,
}

/// Process one XML element and its descendants.
///
/// The reader must be positioned on a `Start` or `Empty` event.
/// On return, the reader is positioned just past this element's end tag
/// (or on the same event for empty elements).
///
/// Mirrors the C# `ChildElementHashGenerator.Generate()` + `GetHashBytesOfCurrentNode()`.
fn process_element(
    reader: &mut Reader<&[u8]>,
    name: &str,
    start: &quick_xml::events::BytesStart<'_>,
    is_empty: bool,
    registry: &HashMap<&str, (usize, &'static ElementInfo)>,
    parent_name: &str,
) -> Result<GenResult, KnxprodError> {
    let mut stream = Vec::new();
    let mut order_key: Option<String> = None;
    let mut order_is_relevant = false;

    if let Some((_, info)) = registry.get(name) {
        let (attr_bytes, sort_val, is_ordered) = serialize_registry_attrs(info, start);
        stream.extend_from_slice(&attr_bytes);
        order_is_relevant = is_ordered;
        // C# ParameterRefRefElementInfo.OrderIsRelevant returns false when
        // parent is LParameters or RParameters.
        if order_is_relevant
            && name == "ParameterRefRef"
            && (parent_name == "LParameters" || parent_name == "RParameters")
        {
            order_is_relevant = false;
        }
        order_key = sort_val;

        if !is_empty {
            match info.kind {
                ElementKind::InnerTextBase64(_)
                | ElementKind::InnerTextUInt32(_)
                | ElementKind::InnerTextString(_) => {
                    let _ = scan_for_inner_text(reader, info.kind, &mut stream)?;
                }
                ElementKind::Attrs(_, _) => {
                    read_children(reader, registry, &mut stream, name)?;
                }
            }
        } else if matches!(
            info.kind,
            ElementKind::InnerTextBase64(_)
                | ElementKind::InnerTextUInt32(_)
                | ElementKind::InnerTextString(_)
        ) {
            let _ = scan_for_inner_text(reader, info.kind, &mut stream)?;
        }
    } else if !is_empty {
        read_children(reader, registry, &mut stream, name)?;
    }

    // If no explicit sort key, use MD5 of bytes (matching C# OrderKey property).
    if order_key.is_none() && !stream.is_empty() {
        let mut h = Md5::new();
        h.update(&stream);
        order_key = Some(base64::engine::general_purpose::STANDARD.encode(h.finalize()));
    }

    Ok(GenResult {
        bytes: stream,
        order_key,
        order_is_relevant,
    })
}

/// Read inner text content for `InnerTextBase64`, `InnerTextUInt32`, and
/// `InnerTextString` elements. Reads until `EndElement`, writing the decoded
/// content to `stream`. The tag byte was already written by
/// `serialize_registry_attrs`.
///
/// Scan forward for the next Text node, returning the depth offset
/// Scan forward for the next Text node.
/// Returns the number of extra End events the caller must absorb.
fn scan_for_inner_text(
    reader: &mut Reader<&[u8]>,
    kind: ElementKind,
    stream: &mut Vec<u8>,
) -> Result<i32, KnxprodError> {
    let mut depth: i32 = 0;
    loop {
        match reader.read_event()? {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            Event::Text(ref t) => {
                let raw = std::str::from_utf8(t.as_ref()).unwrap_or("");
                let unescaped = quick_xml::escape::unescape(raw).unwrap_or_default();
                let normalized = unescaped.replace("\r\n", "\n").replace('\r', "\n");
                if normalized.trim().is_empty() {
                    continue;
                }
                if depth > 0 {
                    write_collected_text(kind, &normalized, stream);
                    return Ok(depth);
                }
                return collect_remaining_text(reader, kind, normalized, stream);
            }
            Event::CData(ref c) => {
                let raw = std::str::from_utf8(c.as_ref()).unwrap_or("");
                let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
                if normalized.trim().is_empty() {
                    continue;
                }
                if depth > 0 {
                    write_collected_text(kind, &normalized, stream);
                    return Ok(depth);
                }
                return collect_remaining_text(reader, kind, normalized, stream);
            }
            Event::GeneralRef(ref r) => {
                let decoded = decode_entity(std::str::from_utf8(r.as_ref()).unwrap_or(""));
                if !decoded.is_empty() {
                    if depth > 0 {
                        write_collected_text(kind, decoded, stream);
                        return Ok(depth);
                    }
                    return collect_remaining_text(reader, kind, String::from(decoded), stream);
                }
            }
            Event::Eof => return Ok(0),
            _ => {}
        }
    }
}

/// Collect remaining text fragments (`Text`, `CData`, `GeneralRef`) until `EndElement`.
/// Used for depth==0 (text inside own element) where `quick_xml` splits at entities.
fn collect_remaining_text(
    reader: &mut Reader<&[u8]>,
    kind: ElementKind,
    mut text_buf: String,
    stream: &mut Vec<u8>,
) -> Result<i32, KnxprodError> {
    loop {
        match reader.read_event()? {
            Event::Text(ref t) => {
                let raw = std::str::from_utf8(t.as_ref()).unwrap_or("");
                let u = quick_xml::escape::unescape(raw).unwrap_or_default();
                text_buf.push_str(&u.replace("\r\n", "\n").replace('\r', "\n"));
            }
            Event::CData(ref c) => {
                let raw = std::str::from_utf8(c.as_ref()).unwrap_or("");
                text_buf.push_str(&raw.replace("\r\n", "\n").replace('\r', "\n"));
            }
            Event::GeneralRef(ref r) => {
                text_buf.push_str(decode_entity(std::str::from_utf8(r.as_ref()).unwrap_or("")));
            }
            _ => {
                write_collected_text(kind, &text_buf, stream);
                return Ok(0);
            }
        }
    }
}

fn decode_entity(entity: &str) -> &'static str {
    match entity {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "apos" => "'",
        "quot" => "\"",
        _ => "",
    }
}

/// Write collected inner text to stream.
fn write_collected_text(kind: ElementKind, text: &str, stream: &mut Vec<u8>) {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        match kind {
            ElementKind::InnerTextBase64(_) => {
                if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(trimmed) {
                    stream.extend_from_slice(&decoded);
                }
            }
            ElementKind::InnerTextUInt32(_) | ElementKind::InnerTextString(_) => {
                write_string(stream, text);
            }
            ElementKind::Attrs(_, _) => {}
        }
    }
}

/// Read children of the current element until `EndElement`.
///
/// Mirrors the inner loop of C# `Generate()`:
/// - Registry children (case 15): serialized directly into `stream`
/// - Non-registry children (case 19): processed recursively, collected in
///   a `SortedDictionary`, appended after all children are read
fn read_children(
    reader: &mut Reader<&[u8]>,
    registry: &HashMap<&str, (usize, &'static ElementInfo)>,
    stream: &mut Vec<u8>,
    parent_name: &str,
) -> Result<(), KnxprodError> {
    // ALL children go through the sorted collection — matching C# behavior
    // where every child element gets its own ChildElementHashGenerator.
    let mut sorted: Vec<(SortKey, Vec<u8>)> = Vec::new();
    let mut order_counter: i32 = 0;
    // Track nesting depth so we only stop at our own EndElement, not at
    // deeply nested ones exposed by InnerText overshoot.
    loop {
        let (e_ref, is_empty) = match reader.read_event()? {
            Event::Start(ref e) => (e.to_owned(), false),
            Event::Empty(ref e) => (e.to_owned(), true),
            Event::End(_) | Event::Eof => break,
            _ => continue,
        };
        let qn = e_ref.name();
        let name = std::str::from_utf8(local_name(qn.as_ref())).unwrap_or("");
        let result = process_element(reader, name, &e_ref, is_empty, registry, parent_name)?;
        if let Some(key) = result.order_key {
            let sk = if result.order_is_relevant {
                let k = SortKey::Ordered(order_counter);
                order_counter += 1;
                k
            } else {
                SortKey::Sorted(key)
            };
            sorted.push((sk, result.bytes));
        }
    }

    sorted.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (_, bytes) in sorted {
        stream.extend_from_slice(&bytes);
    }
    Ok(())
}

/// Serialize a registry element's attributes.
/// Returns `(bytes, sort_key, is_ordered)`.
fn serialize_registry_attrs(
    info: &ElementInfo,
    start: &quick_xml::events::BytesStart<'_>,
) -> (Vec<u8>, Option<String>, bool) {
    match info.kind {
        ElementKind::Attrs(attrs, sort_attr_name) => {
            let mut attr_map: HashMap<Vec<u8>, String> = HashMap::new();
            for attr in start.attributes().flatten() {
                let key = local_name(attr.key.as_ref()).to_vec();
                let val = attr.unescape_value().map_or_else(
                    |_| String::from_utf8_lossy(&attr.value).into_owned(),
                    std::borrow::Cow::into_owned,
                );
                attr_map.insert(key, val);
            }
            let mut sort_key = None;
            if let Some(sa) = sort_attr_name {
                if let Some(val) = attr_map.get(sa.as_bytes()) {
                    sort_key = Some(normalize_appl_prog_id(val));
                }
            }
            let mut buf = Vec::new();
            for a in attrs {
                let raw = attr_map.get(a.xml_name.as_bytes()).map(String::as_str);
                write_attr(&mut buf, a, raw);
            }
            (buf, sort_key, info.ordered)
        }
        ElementKind::InnerTextUInt32(tag)
        | ElementKind::InnerTextString(tag)
        | ElementKind::InnerTextBase64(tag) => (vec![tag], None, false),
    }
}

/// Sort key for the `SortedDictionary`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SortKey {
    Ordered(i32),
    Sorted(String),
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Ordered(a), Self::Ordered(b)) => a.cmp(b),
            (Self::Sorted(a), Self::Sorted(b)) => cmp_invariant(a, b),
            (Self::Ordered(_), Self::Sorted(_)) => std::cmp::Ordering::Less,
            (Self::Sorted(_), Self::Ordered(_)) => std::cmp::Ordering::Greater,
        }
    }
}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Compare strings matching .NET `string.CompareTo` for ASCII.
///
/// .NET uses multi-level Unicode collation:
/// 1. Primary: base letter (case-insensitive), with symbols < digits < letters
/// 2. Secondary (tiebreaker): uppercase < lowercase
///
/// Verified identical on Windows 11 (.NET 9) and macOS (.NET 10).
fn cmp_invariant(a: &str, b: &str) -> std::cmp::Ordering {
    let primary = a
        .chars()
        .map(char_primary_weight)
        .cmp(b.chars().map(char_primary_weight));
    if primary != std::cmp::Ordering::Equal {
        return primary;
    }
    a.chars()
        .map(|c| u8::from(c.is_uppercase()))
        .cmp(b.chars().map(|c| u8::from(c.is_uppercase())))
}

fn char_primary_weight(c: char) -> u16 {
    if c.is_ascii() {
        u16::from(DOTNET_PRIMARY[c as usize])
    } else {
        // Map accented Latin letters to their base letter weight.
        let base = match c {
            'à'..='å' | 'æ' | 'À'..='Å' | 'Æ' => 'a',
            'è'..='ë' | 'È'..='Ë' => 'e',
            'ì'..='ï' | 'Ì'..='Ï' => 'i',
            'ò'..='ö' | 'Ò'..='Ö' => 'o',
            'ù'..='ü' | 'Ù'..='Ü' => 'u',
            'ñ' | 'Ñ' => 'n',
            'ß' => 's',
            'ý' | 'ÿ' | 'Ý' => 'y',
            _ => return 0x100 + (c as u16).min(0xFEFF),
        };
        u16::from(DOTNET_PRIMARY[base as usize])
    }
}

/// .NET `string.CompareTo` primary sort weights for ASCII characters.
/// Case-insensitive for letters (a==A). Symbols and digits have culture-aware order.
#[rustfmt::skip]
const DOTNET_PRIMARY: [u8; 128] = [
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
    1,  7, 11, 23, 33, 24, 22, 10, 12, 13, 19, 27,  4,  3,  9, 20,
   34, 35, 36, 37, 38, 39, 40, 41, 42, 43,  6,  5, 28, 29, 30,  8,
   18, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58,
   59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 14, 21, 15, 26,  2,
   25, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58,
   59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 16, 31, 17, 32,  0,
];

fn serialize_app_program(
    xml: &str,
    registry: &HashMap<&str, (usize, &'static ElementInfo)>,
) -> Result<Vec<u8>, KnxprodError> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let qn = e.name();
                let name = std::str::from_utf8(local_name(qn.as_ref())).unwrap_or("");
                if name == "ApplicationPrograms" {
                    // Create top-level generator for ApplicationPrograms
                    let mut buf = Vec::new();
                    read_children(&mut reader, registry, &mut buf, "ApplicationPrograms")?;
                    return Ok(buf);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(KnxprodError::Xml(e)),
            _ => {}
        }
    }
    Err(KnxprodError::MissingElement("ApplicationPrograms"))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Result of hashing an `ApplicationProgram` XML subtree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppProgramHash {
    /// Raw MD5 digest (16 bytes).
    pub md5: [u8; 16],
    /// 16-bit fingerprint: `(md5[0] << 8) | md5[15]`.
    pub fingerprint: u16,
}

impl AppProgramHash {
    /// The fingerprint as a 4-character uppercase hex string (e.g. `"2412"`).
    #[must_use]
    pub fn fingerprint_hex(&self) -> String {
        format!("{:04X}", self.fingerprint)
    }

    /// The MD5 hash as a Base64 string (for the `Hash` XML attribute).
    #[must_use]
    pub fn hash_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.md5)
    }

    /// The full MD5 digest as a lowercase hex string.
    #[must_use]
    pub fn md5_hex(&self) -> String {
        self.md5.iter().fold(String::with_capacity(32), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
    }
}

/// Compute the registration-relevant hash of an `ApplicationProgram` XML.
///
/// # Errors
///
/// Returns [`KnxprodError`] on XML parse errors or if no `ApplicationProgram`
/// element is found.
pub fn hash_application_program(xml: &str) -> Result<AppProgramHash, KnxprodError> {
    let registry = build_registry();
    let bytes = serialize_app_program(xml, &registry)?;
    let mut hasher = Md5::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let md5: [u8; 16] = digest.into();
    let fingerprint = (u16::from(md5[0]) << 8) | u16::from(md5[15]);
    Ok(AppProgramHash { md5, fingerprint })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_prebytes_match() {
        let xml = include_str!("../tests/fixtures/minimal_app.xml");
        let registry = build_registry();
        let bytes =
            serialize_app_program(xml, &registry).unwrap_or_else(|e| panic!("serialize: {e}"));
        let expected = include_bytes!("../tests/fixtures/minimal_prebytes.bin");
        assert_eq!(bytes.len(), expected.len(), "length mismatch");
        assert_eq!(bytes, expected.as_slice(), "pre-MD5 bytes mismatch");
    }

    #[test]
    fn minimal_golden_vector() {
        let xml = include_str!("../tests/fixtures/minimal_app.xml");
        let result = hash_application_program(xml).unwrap_or_else(|e| panic!("hash: {e}"));
        assert_eq!(result.md5_hex(), "24ab92da12d47b99de1c6728334c7b12");
        assert_eq!(result.fingerprint, 0x2412);
    }

    #[test]
    fn leakage_prebytes_match() {
        let xml = include_str!("../tests/fixtures/leakage_app.xml");
        let registry = build_registry();
        let bytes =
            serialize_app_program(xml, &registry).unwrap_or_else(|e| panic!("serialize: {e}"));
        let expected = include_bytes!("../tests/fixtures/leakage_prebytes.bin");
        assert_eq!(bytes.len(), expected.len());
        assert_eq!(bytes, expected.as_slice());
    }

    #[test]
    fn leakage_golden_vector() {
        let xml = include_str!("../tests/fixtures/leakage_app.xml");
        let result = hash_application_program(xml).unwrap_or_else(|e| panic!("hash: {e}"));
        assert_eq!(result.md5_hex(), "dd03e07cbe5cc31594a44bea21f566af");
        assert_eq!(result.fingerprint_hex(), "DDAF");
    }

    #[test]
    fn normalize_strips_fingerprint() {
        assert_eq!(
            normalize_appl_prog_id("M-0083_A-0001-01-0000"),
            "M-0083_A-0001-01"
        );
        assert_eq!(
            normalize_appl_prog_id("M-0083_A-014F-10-DDAF"),
            "M-0083_A-014F-10"
        );
    }

    #[test]
    fn varint_encoding() {
        let mut buf = Vec::new();
        write_varint(&mut buf, 3);
        assert_eq!(buf, [0x03]);
        buf.clear();
        write_varint(&mut buf, 127);
        assert_eq!(buf, [0x7F]);
        buf.clear();
        write_varint(&mut buf, 128);
        assert_eq!(buf, [0x80, 0x01]);
    }
}

#[cfg(test)]
mod mdt_golden_vectors {
    use super::*;

    #[test]
    fn mdt_akk_switch_actuator() {
        let xml = include_str!("../tests/fixtures/mdt_akk_app.xml");
        let r = hash_application_program(xml).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.md5_hex(), "78221b53250bc27dd9d7c9c7523655e9");
        assert_eq!(r.fingerprint_hex(), "78E9");
    }

    #[test]
    fn mdt_jal_shutter_actuator() {
        let xml = include_str!("../tests/fixtures/mdt_jal_app.xml");
        let r = hash_application_program(xml).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.md5_hex(), "db716bb6c940751b9a73be683d53e799");
        assert_eq!(r.fingerprint_hex(), "DB99");
    }

    #[test]
    fn mdt_be_binary_input() {
        let xml = include_str!("../tests/fixtures/mdt_be_app.xml");
        let r = hash_application_program(xml).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.md5_hex(), "50c41b9956732a5b5594cd7f35c359e1");
        assert_eq!(r.fingerprint_hex(), "50E1");
    }
}

#[cfg(test)]
mod gira_golden_vectors {
    use super::*;

    #[test]
    fn gira_small() {
        let xml = include_str!("../tests/fixtures/gira_small_app.xml");
        let r = hash_application_program(xml).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.md5_hex(), "a29aac8693127ec75932a231a3923869");
        assert_eq!(r.fingerprint_hex(), "A269");
    }

    #[test]
    fn gira_medium() {
        let xml = include_str!("../tests/fixtures/gira_medium_app.xml");
        let r = hash_application_program(xml).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.md5_hex(), "95aa2b9a5b45fa8681f23dddc9ab9842");
        assert_eq!(r.fingerprint_hex(), "9542");
    }

    #[test]
    fn gira_large() {
        let xml = include_str!("../tests/fixtures/gira_large_app.xml");
        let r = hash_application_program(xml).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.md5_hex(), "b3dbaefa5882b3a0f9114bee136b0e6a");
        assert_eq!(r.fingerprint_hex(), "B36A");
    }
}
