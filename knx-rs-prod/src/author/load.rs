// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Code segments (`<Static><Code>`) and load procedures (`<LoadProcedures>`)
//! for the [`AppProgram`](super::AppProgram) authoring model.
//!
//! The load procedure is the ordered "download machine" ETS runs to flash a
//! device: connect, unload/load the tables, write the memory segments, compare
//! the hardware type, disconnect. Segments own `xs:ID` handles (so
//! [`write_parameters`](super::AppProgram::write_parameters)'s `CodeSegment`
//! reference becomes a [`SegmentId`] instead of a hard-coded string); the load
//! steps are a positional list, so they need no ids — mirroring how a
//! `ComObject` has a handle but a load step does not.
//!
//! The [`LoadControl`] variant set covers the standard download machine and is
//! `#[non_exhaustive]`; the exotic task/function-property/property-descriptor
//! steps extend it without breaking callers.

use std::fmt::Write as _;

use super::{AppProgram, escape_attr};

/// `LdCtrlProcType_t` — which portion of the download a step applies to
/// (the `AppliesTo` attribute; `Auto`/`None` omits it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcType {
    /// `full`
    Full,
    /// `par`
    Par,
    /// `grp`
    Grp,
    /// `full,par`
    FullPar,
    /// `full,grp`
    FullGrp,
    /// `par,grp`
    ParGrp,
    /// `all`
    All,
}

impl ProcType {
    const fn ets(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Par => "par",
            Self::Grp => "grp",
            Self::FullPar => "full,par",
            Self::FullGrp => "full,grp",
            Self::ParGrp => "par,grp",
            Self::All => "all",
        }
    }
}

/// `MemoryType_t` for a `<Code>` segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegMemoryType {
    /// `Ram`
    Ram,
    /// `Eeprom`
    Eeprom,
    /// `Flash`
    Flash,
}

impl SegMemoryType {
    const fn ets(self) -> &'static str {
        match self {
            Self::Ram => "Ram",
            Self::Eeprom => "Eeprom",
            Self::Flash => "Flash",
        }
    }
}

/// `LdCtrlErrorCause_t` — the failure an `<OnError>` handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCause {
    /// `ResourceNotFound`
    ResourceNotFound,
    /// `CompareMismatch`
    CompareMismatch,
}

impl ErrorCause {
    const fn ets(self) -> &'static str {
        match self {
            Self::ResourceNotFound => "ResourceNotFound",
            Self::CompareMismatch => "CompareMismatch",
        }
    }
}

/// How a property/table load step names its interface object. Exactly one
/// addressing mode is meaningful, so the enum makes an illegal combo
/// unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjTarget {
    /// `LsmIdx="n"` — a load-state-machine index.
    Lsm(u8),
    /// `ObjType="t"` (+ `Occurrence` when non-zero).
    Object {
        /// Interface-object type.
        obj_type: u16,
        /// Zero-based occurrence (omitted when 0).
        occurrence: u16,
    },
    /// `ObjIdx="i"` (+ `Occurrence` when non-zero) — property steps.
    ObjIdx {
        /// Interface-object index.
        obj_idx: u8,
        /// Zero-based occurrence (omitted when 0).
        occurrence: u16,
    },
}

impl ObjTarget {
    fn attrs(self) -> String {
        let mut s = String::new();
        let occ = match self {
            Self::Lsm(n) => {
                let _ = write!(s, r#" LsmIdx="{n}""#);
                0
            }
            Self::Object {
                obj_type,
                occurrence,
            } => {
                let _ = write!(s, r#" ObjType="{obj_type}""#);
                occurrence
            }
            Self::ObjIdx {
                obj_idx,
                occurrence,
            } => {
                let _ = write!(s, r#" ObjIdx="{obj_idx}""#);
                occurrence
            }
        };
        if occ != 0 {
            let _ = write!(s, r#" Occurrence="{occ}""#);
        }
        s
    }
}

/// An `<OnError>` handler nested in a load step.
#[derive(Clone, Debug)]
pub struct OnError {
    /// The failure this handles.
    pub cause: ErrorCause,
    /// `Ignore="true"` when set (default false → omitted).
    pub ignore: bool,
    /// Optional `MessageRef` id (an app-program `_M-` message).
    pub message_ref: Option<String>,
}

/// Inherited `LdCtrlBase_t` knobs every step may carry.
#[derive(Clone, Debug, Default)]
pub struct StepBase {
    /// `AppliesTo` (omitted when `None`, i.e. `auto`).
    pub applies_to: Option<ProcType>,
    /// `<OnError>` children.
    pub on_error: Vec<OnError>,
}

/// A code segment placed in `<Static><Code>`.
#[derive(Clone, Debug)]
pub enum Segment {
    /// An absolutely-addressed segment (`<AbsoluteSegment>`), id `_AS-<n>`.
    Absolute {
        /// Optional `Name`.
        name: Option<String>,
        /// `Size` in bytes.
        size: u32,
        /// `Address`.
        address: u32,
        /// Optional `MemoryType`.
        memory_type: Option<SegMemoryType>,
        /// `UserMemory` (omitted when false).
        user_memory: bool,
    },
    /// A relatively-addressed segment (`<RelativeSegment>`), id `_RS-<lsm>-<index>`.
    Relative {
        /// Optional `Name`.
        name: Option<String>,
        /// `Size` in bytes.
        size: u32,
        /// `LoadStateMachine` index (also the `_RS-<lsm>` id part).
        load_state_machine: u8,
        /// `Offset`.
        offset: u32,
    },
}

/// Opaque handle to a registered [`Segment`]. Only obtainable from
/// [`AppProgram::add_segment`], so a `CodeSegment` reference can't dangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentId(pub(super) usize);

/// One `<LdCtrl*>` load step. Covers the standard download machine;
/// `#[non_exhaustive]` so the exotic task/function-property steps extend it.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum LoadControl {
    /// `<LdCtrlConnect />`
    Connect,
    /// `<LdCtrlDisconnect />`
    Disconnect,
    /// `<LdCtrlRestart />`
    Restart,
    /// `<LdCtrlUnload>`
    Unload(ObjTarget),
    /// `<LdCtrlLoad>`
    Load(ObjTarget),
    /// `<LdCtrlLoadCompleted>`
    LoadCompleted(ObjTarget),
    /// `<LdCtrlAbsSegment>`
    AbsSegment {
        /// Object addressing.
        target: ObjTarget,
        /// `SegType`.
        seg_type: u8,
        /// `Address`.
        address: u32,
        /// `Size`.
        size: u32,
        /// `Access`.
        access: u8,
        /// `MemType`.
        mem_type: u8,
        /// `SegFlags`.
        seg_flags: u8,
    },
    /// `<LdCtrlRelSegment>`
    RelSegment {
        /// Object addressing.
        target: ObjTarget,
        /// `Size`.
        size: u32,
        /// `Mode`.
        mode: u8,
        /// `Fill`.
        fill: u8,
    },
    /// `<LdCtrlWriteMem>`
    WriteMem {
        /// `Address`.
        address: u32,
        /// `Size`.
        size: u32,
        /// `Verify` (omitted when false).
        verify: bool,
        /// `InlineData` (hex).
        inline_data: Option<Vec<u8>>,
    },
    /// `<LdCtrlLoadImageMem>`
    LoadImageMem {
        /// `Address`.
        address: u32,
        /// `Size`.
        size: u32,
    },
    /// `<LdCtrlWriteRelMem>`
    WriteRelMem {
        /// Object addressing.
        target: ObjTarget,
        /// `Offset`.
        offset: u32,
        /// `Size`.
        size: u32,
        /// `Verify` (omitted when false).
        verify: bool,
        /// `InlineData` (hex).
        inline_data: Option<Vec<u8>>,
    },
    /// `<LdCtrlCompareProp>`
    CompareProp {
        /// Object addressing.
        target: ObjTarget,
        /// `PropId`.
        prop_id: u16,
        /// `InlineData` (hex).
        inline_data: Option<Vec<u8>>,
    },
    /// `<LdCtrlLoadImageProp>`
    LoadImageProp {
        /// Object addressing.
        target: ObjTarget,
        /// `PropId`.
        prop_id: u16,
    },
    /// `<LdCtrlDelay>`
    Delay {
        /// `MilliSeconds`.
        milliseconds: u16,
    },
    /// `<LdCtrlProgressText>`
    ProgressText {
        /// `TextId`.
        text_id: Option<u32>,
        /// `MessageRef` id.
        message_ref: Option<String>,
    },
}

impl LoadControl {
    /// The element name and its own (non-base) attribute string.
    fn element(&self) -> (&'static str, String) {
        match self {
            Self::Connect => ("LdCtrlConnect", String::new()),
            Self::Disconnect => ("LdCtrlDisconnect", String::new()),
            Self::Restart => ("LdCtrlRestart", String::new()),
            Self::Unload(t) => ("LdCtrlUnload", t.attrs()),
            Self::Load(t) => ("LdCtrlLoad", t.attrs()),
            Self::LoadCompleted(t) => ("LdCtrlLoadCompleted", t.attrs()),
            Self::AbsSegment {
                target,
                seg_type,
                address,
                size,
                access,
                mem_type,
                seg_flags,
            } => {
                let mut a = target.attrs();
                let _ = write!(
                    a,
                    r#" SegType="{seg_type}" Address="{address}" Size="{size}" Access="{access}" MemType="{mem_type}" SegFlags="{seg_flags}""#,
                );
                ("LdCtrlAbsSegment", a)
            }
            Self::RelSegment {
                target,
                size,
                mode,
                fill,
            } => {
                let mut a = target.attrs();
                let _ = write!(a, r#" Size="{size}" Mode="{mode}" Fill="{fill}""#);
                ("LdCtrlRelSegment", a)
            }
            Self::WriteMem {
                address,
                size,
                verify,
                inline_data,
            } => {
                let mut a = format!(r#" Address="{address}" Size="{size}""#);
                if *verify {
                    a.push_str(r#" Verify="true""#);
                }
                push_inline(&mut a, inline_data.as_deref());
                ("LdCtrlWriteMem", a)
            }
            Self::LoadImageMem { address, size } => (
                "LdCtrlLoadImageMem",
                format!(r#" Address="{address}" Size="{size}""#),
            ),
            Self::WriteRelMem {
                target,
                offset,
                size,
                verify,
                inline_data,
            } => {
                let mut a = target.attrs();
                let _ = write!(a, r#" Offset="{offset}" Size="{size}""#);
                if *verify {
                    a.push_str(r#" Verify="true""#);
                }
                push_inline(&mut a, inline_data.as_deref());
                ("LdCtrlWriteRelMem", a)
            }
            Self::CompareProp {
                target,
                prop_id,
                inline_data,
            } => {
                // ETS emits InlineData before the object address for CompareProp.
                let mut a = String::new();
                push_inline(&mut a, inline_data.as_deref());
                a.push_str(&target.attrs());
                let _ = write!(a, r#" PropId="{prop_id}""#);
                ("LdCtrlCompareProp", a)
            }
            Self::LoadImageProp { target, prop_id } => {
                let mut a = target.attrs();
                let _ = write!(a, r#" PropId="{prop_id}""#);
                ("LdCtrlLoadImageProp", a)
            }
            Self::Delay { milliseconds } => {
                ("LdCtrlDelay", format!(r#" MilliSeconds="{milliseconds}""#))
            }
            Self::ProgressText {
                text_id,
                message_ref,
            } => {
                let mut a = String::new();
                if let Some(id) = text_id {
                    let _ = write!(a, r#" TextId="{id}""#);
                }
                if let Some(m) = message_ref {
                    let _ = write!(a, r#" MessageRef="{}""#, escape_attr(m));
                }
                ("LdCtrlProgressText", a)
            }
        }
    }
}

fn push_inline(a: &mut String, data: Option<&[u8]>) {
    if let Some(bytes) = data {
        a.push_str(r#" InlineData=""#);
        for b in bytes {
            let _ = write!(a, "{b:02X}");
        }
        a.push('"');
    }
}

/// One `<LoadProcedure>` — an ordered list of steps with per-step base knobs.
#[derive(Clone, Debug, Default)]
pub struct LoadProcedure {
    /// `MergeId` (omitted when `None`).
    pub merge_id: Option<u8>,
    /// The ordered steps.
    pub steps: Vec<(LoadControl, StepBase)>,
}

impl LoadProcedure {
    /// A load procedure with the given `MergeId`.
    #[must_use]
    pub const fn with_merge_id(merge_id: u8) -> Self {
        Self {
            merge_id: Some(merge_id),
            steps: Vec::new(),
        }
    }

    /// Append a step (base knobs default to `AppliesTo=auto`, no `OnError`).
    #[must_use]
    pub fn step(mut self, control: LoadControl) -> Self {
        self.steps.push((control, StepBase::default()));
        self
    }

    /// Append a step carrying explicit base knobs (`AppliesTo` / `OnError`).
    #[must_use]
    pub fn step_with(mut self, control: LoadControl, base: StepBase) -> Self {
        self.steps.push((control, base));
        self
    }
}

impl AppProgram {
    /// Register a code segment; returns a handle usable as a `CodeSegment`
    /// reference (e.g. by a future typed parameter placement).
    pub fn add_segment(&mut self, segment: Segment) -> SegmentId {
        let idx = self.segments.len();
        self.segments.push(segment);
        SegmentId(idx)
    }

    /// Register a load procedure.
    pub fn add_load_procedure(&mut self, procedure: LoadProcedure) {
        self.load_procedures.push(procedure);
    }

    /// The id of a registered segment (`_AS-<n>` / `_RS-<lsm:02>-<index:05>`).
    #[must_use]
    pub fn segment_id(&self, id: SegmentId) -> String {
        match &self.segments[id.0] {
            Segment::Absolute { .. } => format!("{}_AS-{}", self.app_prefix, id.0),
            Segment::Relative {
                load_state_machine, ..
            } => {
                format!("{}_RS-{load_state_machine:02}-{:05}", self.app_prefix, id.0)
            }
        }
    }

    /// Emit `<Code>` (all `<AbsoluteSegment>`s then `<RelativeSegment>`s, per the
    /// XSD sequence) at `indent` spaces. Emits nothing when there are no segments.
    pub fn write_code(&self, indent: usize, out: &mut String) {
        if self.segments.is_empty() {
            return;
        }
        let pad = " ".repeat(indent);
        let child = " ".repeat(indent + 2);
        let _ = writeln!(out, "{pad}<Code>");
        for (which, want_abs) in [("abs", true), ("rel", false)] {
            let _ = which;
            for (i, seg) in self.segments.iter().enumerate() {
                let is_abs = matches!(seg, Segment::Absolute { .. });
                if is_abs != want_abs {
                    continue;
                }
                let id = self.segment_id(SegmentId(i));
                match seg {
                    Segment::Absolute {
                        name,
                        size,
                        address,
                        memory_type,
                        user_memory,
                    } => {
                        let mut a = format!(r#"Id="{id}""#);
                        if let Some(n) = name {
                            let _ = write!(a, r#" Name="{}""#, escape_attr(n));
                        }
                        let _ = write!(a, r#" Size="{size}""#);
                        if let Some(mt) = memory_type {
                            let _ = write!(a, r#" MemoryType="{}""#, mt.ets());
                        }
                        let _ = write!(a, r#" Address="{address}""#);
                        if *user_memory {
                            a.push_str(r#" UserMemory="true""#);
                        }
                        let _ = writeln!(out, "{child}<AbsoluteSegment {a} />");
                    }
                    Segment::Relative {
                        name,
                        size,
                        load_state_machine,
                        offset,
                    } => {
                        let mut a = format!(r#"Id="{id}""#);
                        if let Some(n) = name {
                            let _ = write!(a, r#" Name="{}""#, escape_attr(n));
                        }
                        let _ = write!(
                            a,
                            r#" Offset="{offset}" Size="{size}" LoadStateMachine="{load_state_machine}""#,
                        );
                        let _ = writeln!(out, "{child}<RelativeSegment {a} />");
                    }
                }
            }
        }
        let _ = writeln!(out, "{pad}</Code>");
    }

    /// Emit `<LoadProcedures>` at `indent` spaces. Emits nothing when empty.
    pub fn write_load_procedures(&self, indent: usize, out: &mut String) {
        if self.load_procedures.is_empty() {
            return;
        }
        let pad = " ".repeat(indent);
        let child = " ".repeat(indent + 2);
        let _ = writeln!(out, "{pad}<LoadProcedures>");
        for proc in &self.load_procedures {
            match proc.merge_id {
                Some(m) => {
                    let _ = writeln!(out, r#"{child}<LoadProcedure MergeId="{m}">"#);
                }
                None => {
                    let _ = writeln!(out, "{child}<LoadProcedure>");
                }
            }
            for (control, base) in &proc.steps {
                write_step(indent + 4, control, base, out);
            }
            let _ = writeln!(out, "{child}</LoadProcedure>");
        }
        let _ = writeln!(out, "{pad}</LoadProcedures>");
    }
}

/// Emit a single load step, self-closing unless it carries `<OnError>` children.
fn write_step(indent: usize, control: &LoadControl, base: &StepBase, out: &mut String) {
    let pad = " ".repeat(indent);
    let (name, mut attrs) = control.element();
    if let Some(pt) = base.applies_to {
        let _ = write!(attrs, r#" AppliesTo="{}""#, pt.ets());
    }
    if base.on_error.is_empty() {
        let _ = writeln!(out, "{pad}<{name}{attrs} />");
        return;
    }
    let _ = writeln!(out, "{pad}<{name}{attrs}>");
    let child = " ".repeat(indent + 2);
    for e in &base.on_error {
        let mut a = format!(r#"Cause="{}""#, e.cause.ets());
        if e.ignore {
            a.push_str(r#" Ignore="true""#);
        }
        if let Some(m) = &e.message_ref {
            let _ = write!(a, r#" MessageRef="{}""#, escape_attr(m));
        }
        let _ = writeln!(out, "{child}<OnError {a} />");
    }
    let _ = writeln!(out, "{pad}</{name}>");
}

#[cfg(test)]
mod tests {
    use super::super::AppProgram;
    use super::*;

    #[test]
    fn code_segments_match_ets_bytes() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        app.add_segment(Segment::Relative {
            name: None,
            size: 1024,
            load_state_machine: 4,
            offset: 0,
        });
        let mut out = String::new();
        app.write_code(12, &mut out);
        let expected = concat!(
            "            <Code>\n",
            "              <RelativeSegment Id=\"M-00FA_A-FF01-01-0000_RS-04-00000\" Offset=\"0\" Size=\"1024\" LoadStateMachine=\"4\" />\n",
            "            </Code>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn segment_id_grammar() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        let rs = app.add_segment(Segment::Relative {
            name: None,
            size: 8,
            load_state_machine: 4,
            offset: 0,
        });
        // Matches the string write_parameters() hard-codes as its CodeSegment.
        assert_eq!(app.segment_id(rs), "M-00FA_A-FF01-01-0000_RS-04-00000");
    }

    #[test]
    fn load_procedure_with_on_error_matches_ets_bytes() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        app.add_load_procedure(
            LoadProcedure::with_merge_id(1)
                .step(LoadControl::Connect)
                .step(LoadControl::Load(ObjTarget::Object {
                    obj_type: 3,
                    occurrence: 0,
                }))
                .step_with(
                    LoadControl::CompareProp {
                        target: ObjTarget::ObjIdx {
                            obj_idx: 0,
                            occurrence: 0,
                        },
                        prop_id: 78,
                        inline_data: Some(vec![0x00, 0x01]),
                    },
                    StepBase {
                        applies_to: None,
                        on_error: vec![OnError {
                            cause: ErrorCause::CompareMismatch,
                            ignore: false,
                            message_ref: Some("M-00FA_A-FF01-01-0000_M-1".into()),
                        }],
                    },
                )
                .step(LoadControl::Disconnect),
        );
        let mut out = String::new();
        app.write_load_procedures(12, &mut out);
        let expected = concat!(
            "            <LoadProcedures>\n",
            "              <LoadProcedure MergeId=\"1\">\n",
            "                <LdCtrlConnect />\n",
            "                <LdCtrlLoad ObjType=\"3\" />\n",
            "                <LdCtrlCompareProp InlineData=\"0001\" ObjIdx=\"0\" PropId=\"78\">\n",
            "                  <OnError Cause=\"CompareMismatch\" MessageRef=\"M-00FA_A-FF01-01-0000_M-1\" />\n",
            "                </LdCtrlCompareProp>\n",
            "                <LdCtrlDisconnect />\n",
            "              </LoadProcedure>\n",
            "            </LoadProcedures>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn rel_segment_step_with_applies_to() {
        let mut out = String::new();
        write_step(
            0,
            &LoadControl::RelSegment {
                target: ObjTarget::Lsm(4),
                size: 1024,
                mode: 1,
                fill: 0,
            },
            &StepBase {
                applies_to: Some(ProcType::Full),
                on_error: Vec::new(),
            },
            &mut out,
        );
        assert_eq!(
            out,
            "<LdCtrlRelSegment LsmIdx=\"4\" Size=\"1024\" Mode=\"1\" Fill=\"0\" AppliesTo=\"full\" />\n"
        );
    }
}
