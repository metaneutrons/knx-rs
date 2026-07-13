// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! The `<Dynamic>` section — the ETS parameter/UI tree that decides what a user
//! sees and when.
//!
//! It is a tree of blocks: [`ChannelIndependentBlock`](Dyn::ChannelIndependentBlock)
//! and [`Channel`](Dyn::Channel) group [`ParameterBlock`](Dyn::ParameterBlock)s,
//! which in turn hold [`ParamRefRef`](Dyn::ParamRefRef)/
//! [`ComObjRefRef`](Dyn::ComObjRefRef) pointers into the Static section,
//! [`Separator`](Dyn::Separator) headlines, and [`Choose`](Dyn::Choose)/`when`
//! visibility branches.
//!
//! Every cross-reference is a **typed handle** ([`ParamRefId`](super::ParamRefId)
//! / [`ComObjectRefId`](super::ComObjectRefId)), so a `RefId` can only point at a
//! parameter or object that was actually registered — the dangling-`RefRef`
//! class ETS reports as an opaque `NullReferenceException` is unrepresentable.

use std::fmt::Write as _;

use super::{AppProgram, ComObjectRefId, ParamRefId, escape_attr};

/// A node in the `<Dynamic>` tree.
#[derive(Clone, Debug)]
pub enum Dyn {
    /// `<ChannelIndependentBlock>` — a block not tied to a channel instance.
    ChannelIndependentBlock(Vec<Self>),
    /// `<Channel>` — a repeatable channel (id `_CH-<suffix>`). `number` is a
    /// string key (e.g. `Z01`), not an integer, and is what ETS keys copy/swap on.
    Channel {
        /// The `_CH-<suffix>` id tail.
        suffix: String,
        /// `Number` — a string channel key.
        number: String,
        /// `Name`.
        name: String,
        /// `Text`.
        text: String,
        /// Child nodes.
        children: Vec<Self>,
    },
    /// `<ParameterBlock>` (id `_PB-<suffix>`) — a titled group of parameters.
    ParameterBlock {
        /// The `_PB-<suffix>` id tail.
        suffix: String,
        /// `Name`.
        name: String,
        /// `Text` (its title; may carry `{{n: …}}` templates).
        text: String,
        /// Optional `TextParameterRefId` — the block title tracks this parameter.
        text_param_ref: Option<ParamRefId>,
        /// `ShowInComObjectTree` (omitted when false).
        show_in_com_object_tree: bool,
        /// Child nodes.
        children: Vec<Self>,
    },
    /// `<ParameterRefRef>` — show a registered parameter here.
    ParamRefRef(ParamRefId),
    /// `<ComObjectRefRef>` — show a registered group object here.
    ComObjRefRef(ComObjectRefId),
    /// `<ParameterSeparator>` (id `_PS-<suffix>`) — a headline / divider.
    Separator {
        /// The `_PS-<suffix>` id tail.
        suffix: String,
        /// `Text`.
        text: String,
        /// `UIHint` (e.g. `Headline`).
        ui_hint: String,
    },
    /// `<choose ParamRefId>` — branch the UI on a parameter's value.
    Choose {
        /// The parameter whose value selects a branch.
        param_ref: ParamRefId,
        /// The `<when>` branches.
        whens: Vec<When>,
    },
}

/// A `<when test="…">` branch inside a [`Dyn::Choose`].
#[derive(Clone, Debug)]
pub struct When {
    /// The `test` expression (e.g. `>=1`, `1`), escaped on emit.
    pub test: String,
    /// The nodes shown when the test matches.
    pub children: Vec<Dyn>,
}

impl AppProgram {
    /// Append a root node to the `<Dynamic>` tree.
    pub fn add_dynamic(&mut self, node: Dyn) {
        self.dynamic.push(node);
    }

    /// The `<ParameterRef>` id a [`ParamRefId`] resolves to (`<pid>_R-<pid>`).
    fn param_ref_ref_id(&self, r: ParamRefId) -> String {
        let pid = self.param_id(&self.parameters[r.0]);
        format!("{pid}_R-{pid}")
    }

    /// The `<ComObjectRef>` id a [`ComObjectRefId`] resolves to (`<oid>_R-<number>`).
    fn com_object_ref_ref_id(&self, r: ComObjectRefId) -> String {
        let co = &self.com_objects[r.0];
        format!("{}_R-{}", self.com_object_id(co), co.number)
    }

    /// Emit the `<Dynamic>` block at `indent` spaces. Emits nothing when empty.
    pub fn write_dynamic(&self, indent: usize, out: &mut String) {
        if self.dynamic.is_empty() {
            return;
        }
        let pad = " ".repeat(indent);
        let _ = writeln!(out, "{pad}<Dynamic>");
        for node in &self.dynamic {
            self.write_dyn_node(indent + 2, node, out);
        }
        let _ = writeln!(out, "{pad}</Dynamic>");
    }

    #[allow(clippy::too_many_lines)]
    fn write_dyn_node(&self, indent: usize, node: &Dyn, out: &mut String) {
        let pad = " ".repeat(indent);
        match node {
            Dyn::ChannelIndependentBlock(children) => {
                let _ = writeln!(out, "{pad}<ChannelIndependentBlock>");
                for c in children {
                    self.write_dyn_node(indent + 2, c, out);
                }
                let _ = writeln!(out, "{pad}</ChannelIndependentBlock>");
            }
            Dyn::Channel {
                suffix,
                number,
                name,
                text,
                children,
            } => {
                let _ = writeln!(
                    out,
                    r#"{pad}<Channel Id="{prefix}_CH-{suffix}" Number="{number}" Name="{name}" Text="{text}">"#,
                    prefix = self.app_prefix,
                    number = escape_attr(number),
                    name = escape_attr(name),
                    text = escape_attr(text),
                );
                for c in children {
                    self.write_dyn_node(indent + 2, c, out);
                }
                let _ = writeln!(out, "{pad}</Channel>");
            }
            Dyn::ParameterBlock {
                suffix,
                name,
                text,
                text_param_ref,
                show_in_com_object_tree,
                children,
            } => {
                let mut attrs = format!(
                    r#"Id="{prefix}_PB-{suffix}" Name="{name}" Text="{text}""#,
                    prefix = self.app_prefix,
                    name = escape_attr(name),
                    text = escape_attr(text),
                );
                if let Some(r) = text_param_ref {
                    let _ = write!(
                        attrs,
                        r#" TextParameterRefId="{}""#,
                        self.param_ref_ref_id(*r)
                    );
                }
                if *show_in_com_object_tree {
                    attrs.push_str(r#" ShowInComObjectTree="true""#);
                }
                let _ = writeln!(out, "{pad}<ParameterBlock {attrs}>");
                for c in children {
                    self.write_dyn_node(indent + 2, c, out);
                }
                let _ = writeln!(out, "{pad}</ParameterBlock>");
            }
            Dyn::ParamRefRef(r) => {
                let _ = writeln!(
                    out,
                    r#"{pad}<ParameterRefRef RefId="{}" />"#,
                    self.param_ref_ref_id(*r),
                );
            }
            Dyn::ComObjRefRef(r) => {
                let _ = writeln!(
                    out,
                    r#"{pad}<ComObjectRefRef RefId="{}" />"#,
                    self.com_object_ref_ref_id(*r),
                );
            }
            Dyn::Separator {
                suffix,
                text,
                ui_hint,
            } => {
                let _ = writeln!(
                    out,
                    r#"{pad}<ParameterSeparator Id="{prefix}_PS-{suffix}" Text="{text}" UIHint="{hint}" />"#,
                    prefix = self.app_prefix,
                    text = escape_attr(text),
                    hint = escape_attr(ui_hint),
                );
            }
            Dyn::Choose { param_ref, whens } => {
                let _ = writeln!(
                    out,
                    r#"{pad}<choose ParamRefId="{}">"#,
                    self.param_ref_ref_id(*param_ref),
                );
                let cpad = " ".repeat(indent + 2);
                for w in whens {
                    let _ = writeln!(out, r#"{cpad}<when test="{}">"#, escape_attr(&w.test));
                    for c in &w.children {
                        self.write_dyn_node(indent + 4, c, out);
                    }
                    let _ = writeln!(out, "{cpad}</when>");
                }
                let _ = writeln!(out, "{pad}</choose>");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{AppProgram, ComObject, Dpt, Flags, Parameter};
    use super::*;

    fn app_with_one_of_each() -> (AppProgram, ParamRefId, ComObjectRefId) {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        let (_, pr) = app.add_param(Parameter::new(
            "G000",
            "G_NumZones",
            "NumZones",
            "Anzahl Zonen",
            "10",
            0,
            8,
        ));
        let (_, or) = app.add_com_object(ComObject::new(
            "Z01000",
            "Zone 1 Play",
            1,
            "Play",
            "Play",
            "1 Bit",
            Dpt::new(1, 1),
            Flags::default(),
        ));
        (app, pr, or)
    }

    #[test]
    fn dynamic_tree_matches_ets_bytes() {
        let (mut app, pr, or) = app_with_one_of_each();
        // <Channel> is a sibling of <ChannelIndependentBlock> (both direct
        // <Dynamic> children) — the ETS content model, verified against the XSD.
        app.add_dynamic(Dyn::ChannelIndependentBlock(vec![
            Dyn::ParameterBlock {
                suffix: "General".into(),
                name: "General".into(),
                text: "Allgemein".into(),
                text_param_ref: None,
                show_in_com_object_tree: false,
                children: vec![
                    Dyn::ParamRefRef(pr),
                    Dyn::Separator {
                        suffix: "G-Server".into(),
                        text: "Server".into(),
                        ui_hint: "Headline".into(),
                    },
                ],
            },
            Dyn::Choose {
                param_ref: pr,
                whens: vec![When {
                    test: ">=1".into(),
                    children: vec![Dyn::ComObjRefRef(or)],
                }],
            },
        ]));
        app.add_dynamic(Dyn::Channel {
            suffix: "Z01".into(),
            number: "Z01".into(),
            name: "Zone1".into(),
            text: "Zone 1".into(),
            children: vec![Dyn::ParameterBlock {
                suffix: "Z01".into(),
                name: "Zone1".into(),
                text: "Zone 1".into(),
                text_param_ref: None,
                show_in_com_object_tree: true,
                children: vec![Dyn::ParamRefRef(pr)],
            }],
        });
        let mut out = String::new();
        app.write_dynamic(12, &mut out);
        let expected = concat!(
            "            <Dynamic>\n",
            "              <ChannelIndependentBlock>\n",
            "                <ParameterBlock Id=\"M-00FA_A-FF01-01-0000_PB-General\" Name=\"General\" Text=\"Allgemein\">\n",
            "                  <ParameterRefRef RefId=\"M-00FA_A-FF01-01-0000_UP-G000_R-M-00FA_A-FF01-01-0000_UP-G000\" />\n",
            "                  <ParameterSeparator Id=\"M-00FA_A-FF01-01-0000_PS-G-Server\" Text=\"Server\" UIHint=\"Headline\" />\n",
            "                </ParameterBlock>\n",
            "                <choose ParamRefId=\"M-00FA_A-FF01-01-0000_UP-G000_R-M-00FA_A-FF01-01-0000_UP-G000\">\n",
            "                  <when test=\"&gt;=1\">\n",
            "                    <ComObjectRefRef RefId=\"M-00FA_A-FF01-01-0000_O-Z01000_R-1\" />\n",
            "                  </when>\n",
            "                </choose>\n",
            "              </ChannelIndependentBlock>\n",
            "              <Channel Id=\"M-00FA_A-FF01-01-0000_CH-Z01\" Number=\"Z01\" Name=\"Zone1\" Text=\"Zone 1\">\n",
            "                <ParameterBlock Id=\"M-00FA_A-FF01-01-0000_PB-Z01\" Name=\"Zone1\" Text=\"Zone 1\" ShowInComObjectTree=\"true\">\n",
            "                  <ParameterRefRef RefId=\"M-00FA_A-FF01-01-0000_UP-G000_R-M-00FA_A-FF01-01-0000_UP-G000\" />\n",
            "                </ParameterBlock>\n",
            "              </Channel>\n",
            "            </Dynamic>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn parameter_block_optional_attrs() {
        let (mut app, pr, _) = app_with_one_of_each();
        app.add_dynamic(Dyn::ParameterBlock {
            suffix: "Z01".into(),
            name: "Zone1".into(),
            text: "Zone 1".into(),
            text_param_ref: Some(pr),
            show_in_com_object_tree: true,
            children: vec![],
        });
        let mut out = String::new();
        app.write_dynamic(0, &mut out);
        assert!(
            out.contains(r#"TextParameterRefId="M-00FA_A-FF01-01-0000_UP-G000_R-M-00FA_A-FF01-01-0000_UP-G000" ShowInComObjectTree="true">"#),
            "{out}"
        );
    }
}
