// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Code-first authoring of ETS product XML.
//!
//! This module is the typed model that replaces hand-written
//! `format!("<...>")` string building (as seen in device `xtask`s). It emits
//! the **monolithic product XML** that the rest of `knx-rs-prod`
//! (renumber → hash → split → sign → package) consumes — it reimplements none
//! of that back half.
//!
//! # Why
//!
//! Assembling the product XML by hand is error-prone in ways a typed model
//! makes unrepresentable:
//!
//! * **IDs and cross-references** are string-concatenated; a typo becomes an
//!   opaque `NullReferenceException` on ETS import. Here, every registered
//!   entity hands back an opaque handle ([`ComObjectId`], [`ComObjectRefId`]),
//!   so you can only reference something you actually created, and the `_O-` /
//!   `_R-` id grammar lives in exactly one place.
//! * **Attribute values** are interpolated raw, so one label containing `&`
//!   breaks the XML. Here every value flows through [`escape_attr`] exactly
//!   once, by construction.
//! * **Numbering** is re-derived at each call site. Here the object `Number`
//!   is supplied once (from the firmware's own `*_asap` SSOT) and stored.
//!
//! # Coverage
//!
//! The model spans a whole application program: [`ParameterType`]s,
//! [`Parameter`]s (+ auto-materialised `<ParameterRef>`s), [`ComObject`]s (+
//! `<ComObjectRef>`s), the [`Dyn`] `<Dynamic>` tree, `<Code>` [`Segment`]s and the
//! [`LoadProcedure`] download machine, `<Languages>` translations, [`Baggage`]s,
//! and the manufacturer-level [`Catalog`](CatalogSection)/[`Hardware`]/[`Message`]/
//! [`Options`] sections. [`AppProgram::write_knx_document`] emits the entire product
//! XML — envelope included — from one populated program.
//!
//! Build a program with the builder methods, or declaratively with the
//! [`knxprod!`](macro@crate::knxprod) macro (see [`dsl`]).

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Write as _;

mod document;
pub mod dsl;
mod dynamic;
mod load;
mod product;
mod ptype;
pub use document::ProgramInfo;
pub use dynamic::{Dyn, When};
pub use load::{
    ErrorCause, LoadControl, LoadProcedure, ObjTarget, OnError, ProcType, SegMemoryType, Segment,
    SegmentId, StepBase,
};
pub use product::{
    CatalogItem, CatalogSection, Hardware, Hardware2Program, Message, Options, Product,
    write_address_table, write_association_table,
};
pub use ptype::{ParamTypeKind, ParameterType};

/// The XML text for a boolean attribute value.
pub(crate) const fn xml_bool(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

/// Errors surfaced by [`AppProgram::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorError {
    /// Two group objects were registered with the same `Number` — ETS keys the
    /// address/association tables by number, so a collision silently wins-last.
    DuplicateComObjectNumber(u32),
}

impl std::fmt::Display for AuthorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateComObjectNumber(n) => {
                write!(f, "duplicate ComObject Number {n}")
            }
        }
    }
}

impl std::error::Error for AuthorError {}

/// Escape a string for use inside an XML double-quoted attribute value.
///
/// Borrows unchanged when no escaping is needed (the common case), so authoring
/// clean labels costs nothing and byte-matches raw interpolation.
pub fn escape_attr(s: &str) -> Cow<'_, str> {
    if !s.bytes().any(|b| matches!(b, b'&' | b'<' | b'>' | b'"')) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

/// A KNX datapoint type, rendered as the ETS `DPST-<main>-<sub>` IDREF.
///
/// Being a typed value (rather than a parsed-and-`unwrap_or(0)` string) means a
/// malformed DPT cannot silently degrade into a dangling `DPST-0-0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dpt {
    /// Main number, e.g. `1` for `1.001`.
    pub main: u16,
    /// Sub number, e.g. `1` for `1.001`.
    pub sub: u16,
}

impl Dpt {
    /// Construct a datapoint type from its main and sub numbers.
    pub const fn new(main: u16, sub: u16) -> Self {
        Self { main, sub }
    }

    /// The ETS `DatapointType` IDREF, e.g. `DPST-1-1` (leading zeros dropped).
    pub fn dpst(self) -> String {
        format!("DPST-{}-{}", self.main, self.sub)
    }
}

/// Group-object communication flags. (The four KNX flags are inherently
/// booleans, so the `struct_excessive_bools` lint does not apply.)
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// `ReadFlag`.
    pub read: bool,
    /// `WriteFlag`.
    pub write: bool,
    /// `TransmitFlag`.
    pub transmit: bool,
    /// `UpdateFlag`.
    pub update: bool,
}

impl Flags {
    /// ETS enable/disable string for a flag.
    const fn ets(enabled: bool) -> &'static str {
        if enabled { "Enabled" } else { "Disabled" }
    }
}

/// A language-dependent attribute a `<Translation>` can override for a locale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attr {
    /// `Text` — the primary label of a parameter, com-object, enumeration, …
    Text,
    /// `Name`.
    Name,
    /// `FunctionText` — a com-object's function label.
    FunctionText,
    /// `SuffixText` — a parameter's trailing unit/suffix label.
    SuffixText,
    /// `VisibleDescription` — inline help text.
    VisibleDescription,
    /// `InitialValue`.
    InitialValue,
    /// `Value`.
    Value,
}

impl Attr {
    /// The exact ETS `AttributeName` byte string.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Name => "Name",
            Self::FunctionText => "FunctionText",
            Self::SuffixText => "SuffixText",
            Self::VisibleDescription => "VisibleDescription",
            Self::InitialValue => "InitialValue",
            Self::Value => "Value",
        }
    }
}

/// One `(element, attribute) -> text` override for a single locale.
#[derive(Clone, Debug)]
struct Translation {
    ref_id: String,
    attribute: &'static str,
    text: String,
}

/// A group object — an ETS `<ComObject>`.
#[derive(Clone, Debug)]
pub struct ComObject {
    suffix: String,
    name: String,
    number: u32,
    text: String,
    function_text: String,
    object_size: String,
    dpt: Dpt,
    flags: Flags,
}

impl ComObject {
    /// Create a group object. `suffix` is the `_O-<suffix>` id tail (e.g.
    /// `Z01000`); `number` is the object number (supply it from the firmware's
    /// own numbering SSOT, e.g. `group_objects::zone_asap`).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        suffix: impl Into<String>,
        name: impl Into<String>,
        number: u32,
        text: impl Into<String>,
        function_text: impl Into<String>,
        object_size: impl Into<String>,
        dpt: Dpt,
        flags: Flags,
    ) -> Self {
        Self {
            suffix: suffix.into(),
            name: name.into(),
            number,
            text: text.into(),
            function_text: function_text.into(),
            object_size: object_size.into(),
            dpt,
            flags,
        }
    }
}

/// Opaque handle to a registered [`ComObject`]. Only obtainable from
/// [`AppProgram::add_com_object`], so a reference can never dangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComObjectId(usize);

/// Opaque handle to the `<ComObjectRef>` auto-materialised alongside a
/// [`ComObject`]. This is what the Dynamic section references.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComObjectRefId(usize);

/// A memory-backed parameter — an ETS `<Parameter>` inside a `<Union>`.
///
/// The `<Memory>` placement's byte `offset` is single-sourced from the firmware's
/// memory layout. The `<Union>`'s `SizeInBit` and the `<Parameter>`'s `ParameterType`
/// reference are both taken from the [`ParameterType`] the parameter references (via
/// its [`ParamTypeId`]), so a parameter's stored width can never disagree with its type.
#[derive(Clone, Debug)]
pub struct Parameter {
    suffix: String,
    name: String,
    param_type: ParamTypeId,
    text: String,
    value: String,
    offset: usize,
}

impl Parameter {
    /// Create a memory-backed parameter. `suffix` is the `_UP-<suffix>` id tail
    /// (e.g. `Z01002`); `param_type` is a handle from
    /// [`AppProgram::add_parameter_type`], which supplies both the `_PT-` type
    /// reference and the `SizeInBit`.
    pub fn new(
        suffix: impl Into<String>,
        name: impl Into<String>,
        param_type: ParamTypeId,
        text: impl Into<String>,
        value: impl Into<String>,
        offset: usize,
    ) -> Self {
        Self {
            suffix: suffix.into(),
            name: name.into(),
            param_type,
            text: text.into(),
            value: value.into(),
            offset,
        }
    }
}

/// Opaque handle to a registered [`ParameterType`].
///
/// Returned by [`AppProgram::add_parameter_type`]. A [`Parameter`] carries one so its
/// `SizeInBit` is single-sourced from the type it references, never passed independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamTypeId(usize);

/// Opaque handle to a registered [`Parameter`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamId(usize);

/// Opaque handle to the `<ParameterRef>` auto-materialised alongside a
/// [`Parameter`]. This is what the Dynamic section references.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamRefId(usize);

/// A packable manufacturer asset — an icon pack, help zip, or embedded blob.
///
/// It contributes both a `<Baggage>` node to the manufacturer's `<Baggages>`
/// declaration and a real file (`bytes`) that the packager writes under
/// `M-XXXX/Baggages/<target_path>/<name>` and folds into the signature.
#[derive(Clone, Debug)]
pub struct Baggage {
    target_path: String,
    name: String,
    bytes: Vec<u8>,
    time_info: Option<String>,
    file_integrity: Option<u32>,
}

impl Baggage {
    /// A baggage packed at `target_path`/`name` (a backslash-separated directory,
    /// possibly empty) with `bytes` as its content.
    pub fn new(target_path: impl Into<String>, name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            target_path: target_path.into(),
            name: name.into(),
            bytes,
            time_info: None,
            file_integrity: None,
        }
    }

    /// Set the `FileInfo/@TimeInfo` (.NET round-trip `"O"` timestamp).
    #[must_use]
    pub fn with_time_info(mut self, iso: impl Into<String>) -> Self {
        self.time_info = Some(iso.into());
        self
    }

    /// Set the `@FileIntegrity` CRC-32 (rendered as 8 upper-case hex digits).
    #[must_use]
    pub const fn with_crc32(mut self, crc: u32) -> Self {
        self.file_integrity = Some(crc);
        self
    }

    /// The file content, for the packager.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Opaque handle to a registered [`Baggage`]. Only obtainable from
/// [`AppProgram::add_baggage`], so an `@IconFile` / `RefId` can't dangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaggageRef(usize);

/// Encode one id component the way ETS `Knx.Ets.XmlSigning.Ids.Id.Encode` does:
/// keep `[A-Za-z0-9]` verbatim; render every other character as its UTF-8 bytes,
/// each `".{:02X}"`. (Callers normalise `\` → `/` in paths first, so a path
/// separator becomes `.2F`.)
fn encode_id_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                let _ = write!(out, ".{b:02X}");
            }
        }
    }
    out
}

/// A single ETS application program being authored.
///
/// Registering an entity assigns it a stable id under `app_prefix` and returns
/// opaque handles; serialisation renders the exact ETS element bytes.
pub struct AppProgram {
    app_prefix: String,
    parameters: Vec<Parameter>,
    /// `<ParameterTypes>` declarations, in registration order.
    parameter_types: Vec<ParameterType>,
    com_objects: Vec<ComObject>,
    /// Locales in first-seen order, so the `<Languages>` block is deterministic.
    language_order: Vec<String>,
    /// `(locale, entry)` in insertion order — byte-stable for hashing/signing.
    translations: Vec<(String, Translation)>,
    /// Manufacturer-level assets, in registration order.
    baggages: Vec<Baggage>,
    /// `<Static><Code>` segments, in registration order.
    segments: Vec<Segment>,
    /// `<LoadProcedures>` procedures, in registration order.
    load_procedures: Vec<LoadProcedure>,
    /// `<Dynamic>` tree roots, in order.
    dynamic: Vec<Dyn>,
    /// `<Catalog>` sections, in registration order.
    catalog_sections: Vec<CatalogSection>,
    /// `<Hardware>` entries, in registration order.
    hardware: Vec<Hardware>,
    /// `<Messages>` entries, in registration order.
    messages: Vec<Message>,
    /// `<AddressTable MaxEntries>` (in `<Static>`), if set.
    address_table_max: Option<u32>,
    /// `<AssociationTable MaxEntries>` (in `<Static>`), if set.
    association_table_max: Option<u32>,
    /// `<Options>` (in `<Static>`), if set.
    options: Option<Options>,
}

impl AppProgram {
    /// Start an application program. `app_prefix` is the id root shared by all
    /// its children, e.g. `M-00FA_A-FF01-01-0000`.
    pub fn new(app_prefix: impl Into<String>) -> Self {
        Self {
            app_prefix: app_prefix.into(),
            parameters: Vec::new(),
            parameter_types: Vec::new(),
            com_objects: Vec::new(),
            language_order: Vec::new(),
            translations: Vec::new(),
            baggages: Vec::new(),
            segments: Vec::new(),
            load_procedures: Vec::new(),
            dynamic: Vec::new(),
            catalog_sections: Vec::new(),
            hardware: Vec::new(),
            messages: Vec::new(),
            address_table_max: None,
            association_table_max: None,
            options: None,
        }
    }

    /// The manufacturer id (`M-XXXX`) — the `app_prefix` up to the `_A-` tail.
    fn manufacturer(&self) -> &str {
        self.app_prefix
            .split_once("_A-")
            .map_or(self.app_prefix.as_str(), |(m, _)| m)
    }

    /// Register a manufacturer-level baggage; returns a handle. The file bytes
    /// are retained for the packager (see [`Baggage::bytes`]).
    pub fn add_baggage(&mut self, baggage: Baggage) -> BaggageRef {
        let idx = self.baggages.len();
        self.baggages.push(baggage);
        BaggageRef(idx)
    }

    /// The `_BG-` id of a baggage: `{manufacturer}_BG-{encode(path)}-{encode(name)}`,
    /// with the path's `\` normalised to `/` before encoding.
    fn baggage_id(&self, b: &Baggage) -> String {
        let path = b.target_path.replace('\\', "/");
        format!(
            "{}_BG-{}-{}",
            self.manufacturer(),
            encode_id_component(&path),
            encode_id_component(&b.name),
        )
    }

    /// Emit the manufacturer-level `<Baggages>` declaration at `indent` spaces
    /// (each nesting level +2; `indent = 6` in a monolithic product XML).
    /// Emits nothing when there are no baggages.
    pub fn write_baggages(&self, indent: usize, out: &mut String) {
        if self.baggages.is_empty() {
            return;
        }
        let l0 = " ".repeat(indent);
        let l1 = " ".repeat(indent + 2);
        let l2 = " ".repeat(indent + 4);
        let _ = writeln!(out, "{l0}<Baggages>");
        for b in &self.baggages {
            let mut attrs = format!(
                r#"TargetPath="{path}" Name="{name}""#,
                path = escape_attr(&b.target_path),
                name = escape_attr(&b.name),
            );
            if let Some(crc) = b.file_integrity {
                let _ = write!(attrs, r#" FileIntegrity="{crc:08X}""#);
            }
            let _ = write!(attrs, r#" Id="{id}""#, id = self.baggage_id(b));
            let _ = writeln!(out, "{l1}<Baggage {attrs}>");
            match &b.time_info {
                Some(t) => {
                    let _ = writeln!(
                        out,
                        r#"{l2}<FileInfo TimeInfo="{t}" />"#,
                        t = escape_attr(t)
                    );
                }
                None => {
                    let _ = writeln!(out, "{l2}<FileInfo />");
                }
            }
            let _ = writeln!(out, "{l1}</Baggage>");
        }
        let _ = writeln!(out, "{l0}</Baggages>");
    }

    /// Translate a registered parameter's `attribute` into `language` (a BCP-47
    /// locale such as `en-US`). The `<TranslationElement>` `RefId` is resolved
    /// from the handle, so it can never point at a non-existent parameter.
    pub fn translate_param(
        &mut self,
        language: impl Into<String>,
        param: ParamId,
        attribute: Attr,
        text: impl Into<String>,
    ) {
        let ref_id = self.param_id(&self.parameters[param.0]);
        self.push_translation(language.into(), ref_id, attribute.as_str(), text.into());
    }

    /// Translate a registered group object's `attribute` into `language`.
    pub fn translate_com_object(
        &mut self,
        language: impl Into<String>,
        com_object: ComObjectId,
        attribute: Attr,
        text: impl Into<String>,
    ) {
        let ref_id = self.com_object_id(&self.com_objects[com_object.0]);
        self.push_translation(language.into(), ref_id, attribute.as_str(), text.into());
    }

    /// Escape hatch: translate an arbitrary application-program element by its
    /// full `Id` (e.g. a `ParameterType`, `Enumeration`, or `ParameterBlock`
    /// that has no typed handle yet).
    pub fn translate_raw(
        &mut self,
        language: impl Into<String>,
        ref_id: impl Into<String>,
        attribute: Attr,
        text: impl Into<String>,
    ) {
        self.push_translation(
            language.into(),
            ref_id.into(),
            attribute.as_str(),
            text.into(),
        );
    }

    fn push_translation(
        &mut self,
        language: String,
        ref_id: String,
        attribute: &'static str,
        text: String,
    ) {
        if !self.language_order.contains(&language) {
            self.language_order.push(language.clone());
        }
        self.translations.push((
            language,
            Translation {
                ref_id,
                attribute,
                text,
            },
        ));
    }

    /// Emit the `<Languages>` block at `indent` spaces (each nesting level +2).
    /// In the monolithic product XML this block is a child of `<Manufacturer>`
    /// (sibling of `<ApplicationPrograms>`), so callers pass `indent = 6`.
    /// Emits nothing when there are no translations.
    pub fn write_languages(&self, indent: usize, out: &mut String) {
        if self.translations.is_empty() {
            return;
        }
        let l0 = " ".repeat(indent);
        let l1 = " ".repeat(indent + 2);
        let l2 = " ".repeat(indent + 4);
        let l3 = " ".repeat(indent + 6);
        let l4 = " ".repeat(indent + 8);
        let _ = writeln!(out, "{l0}<Languages>");
        for lang in &self.language_order {
            let _ = writeln!(
                out,
                r#"{l1}<Language Identifier="{id}">"#,
                id = escape_attr(lang)
            );
            let _ = writeln!(
                out,
                r#"{l2}<TranslationUnit RefId="{prefix}">"#,
                prefix = self.app_prefix
            );
            // This language's referenced element ids, in first-seen order.
            let mut order: Vec<&str> = Vec::new();
            for (l, t) in &self.translations {
                if l == lang && !order.contains(&t.ref_id.as_str()) {
                    order.push(&t.ref_id);
                }
            }
            for ref_id in order {
                let _ = writeln!(out, r#"{l3}<TranslationElement RefId="{ref_id}">"#);
                for (l, t) in &self.translations {
                    if l == lang && t.ref_id == ref_id {
                        let _ = writeln!(
                            out,
                            r#"{l4}<Translation AttributeName="{attr}" Text="{text}" />"#,
                            attr = t.attribute,
                            text = escape_attr(&t.text),
                        );
                    }
                }
                let _ = writeln!(out, "{l3}</TranslationElement>");
            }
            let _ = writeln!(out, "{l2}</TranslationUnit>");
            let _ = writeln!(out, "{l1}</Language>");
        }
        let _ = writeln!(out, "{l0}</Languages>");
    }

    /// Register a memory-backed parameter. Returns a handle to it and to its
    /// automatically-materialised 1:1 `<ParameterRef>`.
    pub fn add_param(&mut self, parameter: Parameter) -> (ParamId, ParamRefId) {
        let idx = self.parameters.len();
        self.parameters.push(parameter);
        (ParamId(idx), ParamRefId(idx))
    }

    /// The full `_UP-` id of a registered parameter.
    fn param_id(&self, p: &Parameter) -> String {
        format!("{}_UP-{}", self.app_prefix, p.suffix)
    }

    /// Emit the `<Parameters>` block at `indent` spaces. Each parameter is a
    /// `<Union>` wrapping its `<Memory>` placement and the `<Parameter>` itself.
    pub fn write_parameters(&self, indent: usize, out: &mut String) {
        let pad = " ".repeat(indent);
        let union = " ".repeat(indent + 2);
        let inner = " ".repeat(indent + 4);
        let _ = writeln!(out, "{pad}<Parameters>");
        for p in &self.parameters {
            // The `<Union>` size and the `_PT-` type reference both come from the
            // referenced ParameterType, so a parameter can't disagree with its type.
            let ty = &self.parameter_types[p.param_type.0];
            let _ = writeln!(
                out,
                r#"{union}<Union SizeInBit="{bits}">"#,
                bits = ty.size_bits()
            );
            let _ = writeln!(
                out,
                r#"{inner}<Memory CodeSegment="{prefix}_RS-04-00000" Offset="{offset}" BitOffset="0" />"#,
                prefix = self.app_prefix,
                offset = p.offset,
            );
            let _ = writeln!(
                out,
                concat!(
                    r#"{inner}<Parameter Id="{id}" Name="{name}" Offset="0" BitOffset="0" "#,
                    r#"ParameterType="{prefix}_PT-{pt}" Text="{text}" Value="{value}" />"#,
                ),
                inner = inner,
                id = self.param_id(p),
                name = escape_attr(&p.name),
                prefix = self.app_prefix,
                pt = ty.name(),
                text = escape_attr(&p.text),
                value = escape_attr(&p.value),
            );
            let _ = writeln!(out, "{union}</Union>");
        }
        let _ = writeln!(out, "{pad}</Parameters>");
    }

    /// Emit the `<ParameterRefs>` block at `indent` spaces — one 1:1
    /// `<ParameterRef>` per registered parameter (`<id>_R-<id>`), derived from
    /// the model rather than re-scraped from the emitted XML.
    pub fn write_parameter_refs(&self, indent: usize, out: &mut String) {
        let pad = " ".repeat(indent);
        let child = " ".repeat(indent + 2);
        let _ = writeln!(out, "{pad}<ParameterRefs>");
        for p in &self.parameters {
            let id = self.param_id(p);
            let _ = writeln!(
                out,
                r#"{child}<ParameterRef Id="{id}_R-{id}" RefId="{id}" />"#
            );
        }
        let _ = writeln!(out, "{pad}</ParameterRefs>");
    }

    /// Register a group object. Returns a handle to the object and to its
    /// automatically-materialised 1:1 `<ComObjectRef>`.
    pub fn add_com_object(&mut self, com_object: ComObject) -> (ComObjectId, ComObjectRefId) {
        let idx = self.com_objects.len();
        self.com_objects.push(com_object);
        (ComObjectId(idx), ComObjectRefId(idx))
    }

    /// The full `_O-` id of a registered object.
    fn com_object_id(&self, co: &ComObject) -> String {
        format!("{}_O-{}", self.app_prefix, co.suffix)
    }

    /// Emit the `<ComObjectTable>` block at `indent` spaces (children at
    /// `indent + 2`), one element per line — matching the ETS layout the
    /// line-oriented back half (`renumber`/`sanity`) expects.
    pub fn write_com_object_table(&self, indent: usize, out: &mut String) {
        let pad = " ".repeat(indent);
        let child = " ".repeat(indent + 2);
        let _ = writeln!(out, "{pad}<ComObjectTable>");
        for co in &self.com_objects {
            let _ = writeln!(
                out,
                concat!(
                    r#"{child}<ComObject Id="{id}" Name="{name}" Number="{number}" "#,
                    r#"Text="{text}" FunctionText="{ft}" ObjectSize="{size}" "#,
                    r#"DatapointType="{dpt}" Priority="Low" ReadFlag="{read}" "#,
                    r#"WriteFlag="{write}" CommunicationFlag="Enabled" "#,
                    r#"TransmitFlag="{transmit}" UpdateFlag="{update}" ReadOnInitFlag="Disabled" />"#,
                ),
                child = child,
                id = self.com_object_id(co),
                name = escape_attr(&co.name),
                number = co.number,
                text = escape_attr(&co.text),
                ft = escape_attr(&co.function_text),
                size = escape_attr(&co.object_size),
                dpt = co.dpt.dpst(),
                read = Flags::ets(co.flags.read),
                write = Flags::ets(co.flags.write),
                transmit = Flags::ets(co.flags.transmit),
                update = Flags::ets(co.flags.update),
            );
        }
        let _ = writeln!(out, "{pad}</ComObjectTable>");
    }

    /// Emit the `<ComObjectRefs>` block at `indent` spaces. Each ref id is
    /// `<objectId>_R-<number>` — the single place the com-object ref grammar
    /// lives, derived from the stored object rather than re-computed.
    pub fn write_com_object_refs(&self, indent: usize, out: &mut String) {
        let pad = " ".repeat(indent);
        let child = " ".repeat(indent + 2);
        let _ = writeln!(out, "{pad}<ComObjectRefs>");
        for co in &self.com_objects {
            let id = self.com_object_id(co);
            let _ = writeln!(
                out,
                r#"{child}<ComObjectRef Id="{id}_R-{number}" RefId="{id}" />"#,
                child = child,
                id = id,
                number = co.number,
            );
        }
        let _ = writeln!(out, "{pad}</ComObjectRefs>");
    }

    /// Check invariants that ETS relies on but does not surface clearly:
    /// group-object numbers must be unique.
    ///
    /// # Errors
    ///
    /// Returns [`AuthorError::DuplicateComObjectNumber`] if two group objects
    /// share the same `Number`.
    pub fn validate(&self) -> Result<(), AuthorError> {
        let mut seen = HashSet::with_capacity(self.com_objects.len());
        for co in &self.com_objects {
            if !seen.insert(co.number) {
                return Err(AuthorError::DuplicateComObjectNumber(co.number));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AppProgram {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        app.add_com_object(ComObject::new(
            "Z01000",
            "Zone 1 Control/Status",
            1,
            "Steuerung/Status",
            "Control/Status",
            "1 Bit",
            Dpt::new(1, 1),
            Flags {
                read: true,
                write: true,
                transmit: true,
                update: true,
            },
        ));
        app
    }

    #[test]
    fn com_object_table_matches_ets_bytes() {
        let mut out = String::new();
        sample().write_com_object_table(12, &mut out);
        let expected = concat!(
            "            <ComObjectTable>\n",
            "              <ComObject Id=\"M-00FA_A-FF01-01-0000_O-Z01000\" Name=\"Zone 1 Control/Status\" ",
            "Number=\"1\" Text=\"Steuerung/Status\" FunctionText=\"Control/Status\" ObjectSize=\"1 Bit\" ",
            "DatapointType=\"DPST-1-1\" Priority=\"Low\" ReadFlag=\"Enabled\" WriteFlag=\"Enabled\" ",
            "CommunicationFlag=\"Enabled\" TransmitFlag=\"Enabled\" UpdateFlag=\"Enabled\" ReadOnInitFlag=\"Disabled\" />\n",
            "            </ComObjectTable>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn com_object_refs_match_ets_bytes() {
        let mut out = String::new();
        sample().write_com_object_refs(12, &mut out);
        let expected = concat!(
            "            <ComObjectRefs>\n",
            "              <ComObjectRef Id=\"M-00FA_A-FF01-01-0000_O-Z01000_R-1\" ",
            "RefId=\"M-00FA_A-FF01-01-0000_O-Z01000\" />\n",
            "            </ComObjectRefs>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn attribute_values_are_escaped() {
        assert_eq!(escape_attr("plain"), "plain");
        assert_eq!(
            escape_attr("Quellcode & Support"),
            "Quellcode &amp; Support"
        );
        assert_eq!(escape_attr(r#"a<b>"c""#), "a&lt;b&gt;&quot;c&quot;");
    }

    #[test]
    fn dpt_drops_leading_zeros() {
        assert_eq!(Dpt::new(1, 1).dpst(), "DPST-1-1");
        assert_eq!(Dpt::new(5, 1).dpst(), "DPST-5-1");
    }

    #[test]
    fn parameters_match_ets_bytes() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        let percent =
            app.add_parameter_type(ParameterType::number("Percent", 8, "unsignedInt", 0, 100));
        app.add_param(Parameter::new(
            "Z01002",
            "Z01_DefVol",
            percent,
            "Standard-Lautstärke",
            "50",
            5,
        ));
        let mut out = String::new();
        app.write_parameters(12, &mut out);
        let expected = concat!(
            "            <Parameters>\n",
            "              <Union SizeInBit=\"8\">\n",
            "                <Memory CodeSegment=\"M-00FA_A-FF01-01-0000_RS-04-00000\" Offset=\"5\" BitOffset=\"0\" />\n",
            "                <Parameter Id=\"M-00FA_A-FF01-01-0000_UP-Z01002\" Name=\"Z01_DefVol\" Offset=\"0\" ",
            "BitOffset=\"0\" ParameterType=\"M-00FA_A-FF01-01-0000_PT-Percent\" Text=\"Standard-Lautstärke\" Value=\"50\" />\n",
            "              </Union>\n",
            "            </Parameters>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn parameter_refs_are_self_referential_1to1() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        let num_zones =
            app.add_parameter_type(ParameterType::number("NumZones", 8, "unsignedInt", 1, 10));
        app.add_param(Parameter::new(
            "G000",
            "G_NumZones",
            num_zones,
            "Anzahl Zonen",
            "10",
            0,
        ));
        let mut out = String::new();
        app.write_parameter_refs(12, &mut out);
        let id = "M-00FA_A-FF01-01-0000_UP-G000";
        let expected = format!(
            "            <ParameterRefs>\n              <ParameterRef Id=\"{id}_R-{id}\" RefId=\"{id}\" />\n            </ParameterRefs>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn languages_block_matches_ets_bytes() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        let percent =
            app.add_parameter_type(ParameterType::number("Percent", 8, "unsignedInt", 0, 100));
        let (p, _) = app.add_param(Parameter::new(
            "Z01002",
            "Z01_DefVol",
            percent,
            "Standard-Lautstärke",
            "50",
            5,
        ));
        app.translate_param("en-US", p, Attr::Text, "Default Volume");
        let mut out = String::new();
        app.write_languages(6, &mut out);
        let expected = concat!(
            "      <Languages>\n",
            "        <Language Identifier=\"en-US\">\n",
            "          <TranslationUnit RefId=\"M-00FA_A-FF01-01-0000\">\n",
            "            <TranslationElement RefId=\"M-00FA_A-FF01-01-0000_UP-Z01002\">\n",
            "              <Translation AttributeName=\"Text\" Text=\"Default Volume\" />\n",
            "            </TranslationElement>\n",
            "          </TranslationUnit>\n",
            "        </Language>\n",
            "      </Languages>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn no_translations_emits_no_languages_block() {
        let app = AppProgram::new("M-00FA_A-FF01-01-0000");
        let mut out = String::new();
        app.write_languages(6, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn baggages_block_matches_ets_bytes() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        // path written with backslashes; id path is /-normalised to .2F, name '.' -> .2E
        app.add_baggage(
            Baggage::new("AE\\2A\\21", "Icons.zip", vec![])
                .with_time_info("2026-03-02T18:55:32.8617333Z"),
        );
        let mut out = String::new();
        app.write_baggages(6, &mut out);
        let expected = concat!(
            "      <Baggages>\n",
            "        <Baggage TargetPath=\"AE\\2A\\21\" Name=\"Icons.zip\" Id=\"M-00FA_BG-AE.2F2A.2F21-Icons.2Ezip\">\n",
            "          <FileInfo TimeInfo=\"2026-03-02T18:55:32.8617333Z\" />\n",
            "        </Baggage>\n",
            "      </Baggages>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn baggage_id_encoding_matches_ets_examples() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        app.add_baggage(Baggage::new("", "ets.png", vec![]));
        app.add_baggage(Baggage::new("", "Help_de.zip", vec![]));
        let mut out = String::new();
        app.write_baggages(6, &mut out);
        // '.' -> .2E, '_' -> .5F (verified against SmartHomeBridge.knxprod)
        assert!(out.contains(r#"Id="M-00FA_BG--ets.2Epng""#), "{out}");
        assert!(out.contains(r#"Id="M-00FA_BG--Help.5Fde.2Ezip""#), "{out}");
    }

    #[test]
    fn duplicate_com_object_number_is_rejected() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        let co = |n| {
            ComObject::new(
                "X",
                "x",
                n,
                "x",
                "x",
                "1 Bit",
                Dpt::new(1, 1),
                Flags::default(),
            )
        };
        app.add_com_object(co(1));
        app.add_com_object(co(1));
        assert_eq!(
            app.validate(),
            Err(AuthorError::DuplicateComObjectNumber(1))
        );
    }
}
