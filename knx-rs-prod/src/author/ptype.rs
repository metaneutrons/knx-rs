// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! `<ParameterType>` declarations — the value domains parameters draw from.
//!
//! A [`ParameterType`] names a reusable domain (id `_PT-<name>`) that a
//! [`Parameter`](super::Parameter) references by name. Three shapes are modelled:
//! free [`Text`](ParamTypeKind::Text), a bounded [`Number`](ParamTypeKind::Number),
//! and a value [`Enumeration`](ParamTypeKind::Enumeration). Each carries its own
//! `SizeInBit`, so a type is the single source of the width of every parameter that
//! references it.

use std::fmt::Write as _;

use super::{AppProgram, ParamTypeId, escape_attr};

/// The value domain of a [`ParameterType`].
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ParamTypeKind {
    /// `<TypeText SizeInBit>` — a free-text field.
    Text {
        /// `SizeInBit`.
        size_bits: u16,
    },
    /// `<TypeNumber SizeInBit Type minInclusive maxInclusive>` — a bounded integer.
    Number {
        /// `SizeInBit`.
        size_bits: u16,
        /// `Type` (e.g. `unsignedInt`).
        number_type: String,
        /// `minInclusive`.
        min: i64,
        /// `maxInclusive`.
        max: i64,
    },
    /// `<TypeRestriction Base="Value" SizeInBit>` with `<Enumeration>` children.
    Enumeration {
        /// `SizeInBit`.
        size_bits: u16,
        /// The `(Text, Value)` pairs in declaration order; the `_EN-<i>` id is the
        /// 0-based position.
        values: Vec<(String, i64)>,
    },
}

impl ParamTypeKind {
    /// The `SizeInBit` this domain occupies.
    #[must_use]
    pub const fn size_bits(&self) -> u16 {
        match self {
            Self::Text { size_bits }
            | Self::Number { size_bits, .. }
            | Self::Enumeration { size_bits, .. } => *size_bits,
        }
    }
}

/// A `<ParameterType>` (id `_PT-<name>`).
#[derive(Clone, Debug)]
pub struct ParameterType {
    name: String,
    kind: ParamTypeKind,
}

impl ParameterType {
    /// A free-text parameter type of `size_bits` bits (`<TypeText>`).
    #[must_use]
    pub fn text(name: impl Into<String>, size_bits: u16) -> Self {
        Self {
            name: name.into(),
            kind: ParamTypeKind::Text { size_bits },
        }
    }

    /// A bounded integer parameter type (`<TypeNumber>`).
    #[must_use]
    pub fn number(
        name: impl Into<String>,
        size_bits: u16,
        number_type: impl Into<String>,
        min: i64,
        max: i64,
    ) -> Self {
        Self {
            name: name.into(),
            kind: ParamTypeKind::Number {
                size_bits,
                number_type: number_type.into(),
                min,
                max,
            },
        }
    }

    /// An enumerated parameter type; each `(text, value)` becomes an `<Enumeration>`
    /// (`<TypeRestriction Base="Value">`).
    #[must_use]
    pub fn enumeration(
        name: impl Into<String>,
        size_bits: u16,
        values: Vec<(String, i64)>,
    ) -> Self {
        Self {
            name: name.into(),
            kind: ParamTypeKind::Enumeration { size_bits, values },
        }
    }

    /// This type's name (its `_PT-` id tail).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// This type's value domain.
    #[must_use]
    pub const fn kind(&self) -> &ParamTypeKind {
        &self.kind
    }

    /// The `SizeInBit` of every parameter that references this type.
    #[must_use]
    pub const fn size_bits(&self) -> u16 {
        self.kind.size_bits()
    }
}

impl AppProgram {
    /// Register a `<ParameterType>` and return a handle to it. Types are emitted in
    /// registration order; the handle is what a [`Parameter`](super::Parameter) carries
    /// to draw its `SizeInBit` and `_PT-` reference from this type.
    pub fn add_parameter_type(&mut self, parameter_type: ParameterType) -> ParamTypeId {
        let idx = self.parameter_types.len();
        self.parameter_types.push(parameter_type);
        ParamTypeId(idx)
    }

    /// Emit the `<ParameterTypes>` block at `indent` spaces (each nesting level +2).
    /// Emits nothing when no types were registered.
    pub fn write_parameter_types(&self, indent: usize, out: &mut String) {
        if self.parameter_types.is_empty() {
            return;
        }
        let l0 = " ".repeat(indent);
        let l1 = " ".repeat(indent + 2);
        let l2 = " ".repeat(indent + 4);
        let l3 = " ".repeat(indent + 6);
        let _ = writeln!(out, "{l0}<ParameterTypes>");
        for pt in &self.parameter_types {
            let id = format!("{}_PT-{}", self.app_prefix, pt.name);
            let _ = writeln!(
                out,
                r#"{l1}<ParameterType Id="{id}" Name="{name}">"#,
                name = escape_attr(&pt.name),
            );
            match &pt.kind {
                ParamTypeKind::Text { size_bits } => {
                    let _ = writeln!(out, r#"{l2}<TypeText SizeInBit="{size_bits}" />"#);
                }
                ParamTypeKind::Number {
                    size_bits,
                    number_type,
                    min,
                    max,
                } => {
                    let _ = writeln!(
                        out,
                        r#"{l2}<TypeNumber SizeInBit="{size_bits}" Type="{number_type}" minInclusive="{min}" maxInclusive="{max}" />"#,
                        number_type = escape_attr(number_type),
                    );
                }
                ParamTypeKind::Enumeration { size_bits, values } => {
                    let _ = writeln!(
                        out,
                        r#"{l2}<TypeRestriction Base="Value" SizeInBit="{size_bits}">"#
                    );
                    for (i, (text, value)) in values.iter().enumerate() {
                        let _ = writeln!(
                            out,
                            r#"{l3}<Enumeration Text="{text}" Value="{value}" Id="{id}_EN-{i}" />"#,
                            text = escape_attr(text),
                        );
                    }
                    let _ = writeln!(out, "{l2}</TypeRestriction>");
                }
            }
            let _ = writeln!(out, "{l1}</ParameterType>");
        }
        let _ = writeln!(out, "{l0}</ParameterTypes>");
    }
}

#[cfg(test)]
mod tests {
    use super::super::AppProgram;
    use super::*;

    #[test]
    fn parameter_types_match_ets_bytes() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        app.add_parameter_type(ParameterType::enumeration(
            "YesNo",
            8,
            vec![("Nein".into(), 0), ("Ja".into(), 1)],
        ));
        app.add_parameter_type(ParameterType::text("Name", 160));
        app.add_parameter_type(ParameterType::number("Percent", 8, "unsignedInt", 0, 100));
        let mut out = String::new();
        app.write_parameter_types(12, &mut out);
        let expected = concat!(
            "            <ParameterTypes>\n",
            "              <ParameterType Id=\"M-00FA_A-FF01-01-0000_PT-YesNo\" Name=\"YesNo\">\n",
            "                <TypeRestriction Base=\"Value\" SizeInBit=\"8\">\n",
            "                  <Enumeration Text=\"Nein\" Value=\"0\" Id=\"M-00FA_A-FF01-01-0000_PT-YesNo_EN-0\" />\n",
            "                  <Enumeration Text=\"Ja\" Value=\"1\" Id=\"M-00FA_A-FF01-01-0000_PT-YesNo_EN-1\" />\n",
            "                </TypeRestriction>\n",
            "              </ParameterType>\n",
            "              <ParameterType Id=\"M-00FA_A-FF01-01-0000_PT-Name\" Name=\"Name\">\n",
            "                <TypeText SizeInBit=\"160\" />\n",
            "              </ParameterType>\n",
            "              <ParameterType Id=\"M-00FA_A-FF01-01-0000_PT-Percent\" Name=\"Percent\">\n",
            "                <TypeNumber SizeInBit=\"8\" Type=\"unsignedInt\" minInclusive=\"0\" maxInclusive=\"100\" />\n",
            "              </ParameterType>\n",
            "            </ParameterTypes>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn size_bits_reads_through_the_kind() {
        assert_eq!(ParameterType::text("T", 160).size_bits(), 160);
        assert_eq!(
            ParameterType::number("N", 16, "unsignedInt", 0, 65535).size_bits(),
            16
        );
        assert_eq!(
            ParameterType::enumeration("E", 8, vec![("a".into(), 0)]).size_bits(),
            8
        );
    }

    #[test]
    fn empty_emits_nothing() {
        let app = AppProgram::new("M-00FA_A-FF01-01-0000");
        let mut out = String::new();
        app.write_parameter_types(12, &mut out);
        assert!(out.is_empty());
    }
}
