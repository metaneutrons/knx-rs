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
//! # Status
//!
//! This is the Phase 1 foundation of a phased migration: the escaping writer,
//! the typed model, and the `ComObject` / `ComObjectRef` pass (the first
//! section strangled out of the hand-written generator). Further sections
//! (parameters, dynamic, load procedures, catalog, hardware) land in later
//! phases behind the same model.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Write as _;

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

/// A single ETS application program being authored.
///
/// Registering an entity assigns it a stable id under `app_prefix` and returns
/// opaque handles; serialisation renders the exact ETS element bytes.
pub struct AppProgram {
    app_prefix: String,
    com_objects: Vec<ComObject>,
}

impl AppProgram {
    /// Start an application program. `app_prefix` is the id root shared by all
    /// its children, e.g. `M-00FA_A-FF01-01-0000`.
    pub fn new(app_prefix: impl Into<String>) -> Self {
        Self {
            app_prefix: app_prefix.into(),
            com_objects: Vec::new(),
        }
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
