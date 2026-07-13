// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Pure-Rust structural sanity checks for KNX product XML.
//!
//! These are the checks that turn a cryptic ETS *import-time* failure into an
//! actionable *build-time* error. They mirror the relevant parts of
//! `OpenKNXproducer`'s `ProcessSanityChecks` and cover exactly the failure
//! classes seen while making snapdog's `.knxprod` importable:
//!
//! 1. **Id format** — every `ApplicationProgram`-scoped `_P-`/`_UP-`/`_O-`/
//!    `_R-`/`_PB-`/`_PS-` suffix must be a base-10 integer (`'G' is not a legal
//!    digit for base 10`). Run [`renumber_ids`](crate::renumber::renumber_ids)
//!    first.
//! 2. **Reference integrity** — every `RefId`/`ParamRefId`/`…RefId` that points
//!    into the `ApplicationProgram` must resolve to a declared `Id` (a dangling
//!    ref is a `NullReferenceException` on import).
//! 3. **Uniqueness** — no `Id` is declared twice.
//!
//! Deliberately *not* an XSD validator: it is dependency-free and catches the
//! runtime rules the XSD does not encode. XSD validation stays an opt-in,
//! caller-supplied gate (the schema is ETS-proprietary and never bundled).

use std::collections::HashSet;

use crate::error::KnxprodError;
use crate::parse;
use crate::xml_scan;

/// Run every structural sanity check against `xml`.
///
/// # Errors
///
/// Returns [`KnxprodError::Validation`] with one line per problem found, or
/// [`KnxprodError::MissingElement`] / [`KnxprodError::Xml`] if the
/// `ApplicationProgram` id cannot be extracted.
pub fn sanity_check(xml: &str) -> Result<(), KnxprodError> {
    let app_id = parse::extract_application_id(xml)?;
    let prefix = format!("{app_id}_");

    let mut declared: HashSet<String> = HashSet::new();
    let mut dup: Vec<String> = Vec::new();
    let mut bad_format: Vec<String> = Vec::new();
    let mut references: Vec<String> = Vec::new();

    for tag in xml_scan::open_tags(xml) {
        for (name, value) in xml_scan::parse_attrs(tag.body) {
            let in_scope = value.starts_with(&prefix);
            if !in_scope {
                continue;
            }
            if name == "Id" {
                if !declared.insert(value.to_string()) {
                    dup.push(value.to_string());
                }
                if let Some(bad) = bad_integer_suffix(value, &prefix) {
                    bad_format.push(bad);
                }
            } else if is_reference_attr(name) {
                references.push(value.to_string());
                // A ref id embeds an in-scope suffix too — validate its format.
                if let Some(bad) = bad_integer_suffix(value, &prefix) {
                    bad_format.push(bad);
                }
            }
        }
    }

    let mut errors: Vec<String> = Vec::new();

    bad_format.sort();
    bad_format.dedup();
    for b in &bad_format {
        errors.push(format!("non-integer id suffix: {b}"));
    }

    dup.sort();
    dup.dedup();
    for d in &dup {
        errors.push(format!("duplicate Id: {d}"));
    }

    let mut dangling: Vec<String> = references
        .into_iter()
        .filter(|r| !declared.contains(r))
        .collect();
    dangling.sort();
    dangling.dedup();
    for d in &dangling {
        errors.push(format!("dangling reference (no such Id): {d}"));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        // Cap the report so a systemic slip does not print thousands of lines.
        const MAX: usize = 40;
        let total = errors.len();
        if total > MAX {
            errors.truncate(MAX);
            errors.push(format!("… and {} more", total - MAX));
        }
        Err(KnxprodError::Validation(errors.join("\n")))
    }
}

/// Type markers whose suffix ETS parses as a base-10 integer.
const INT_MARKERS: [&str; 6] = ["_UP-", "_P-", "_O-", "_R-", "_PB-", "_PS-"];

/// Return the offending `marker+suffix` if `value` is an *integer-typed* id whose
/// suffix (or an embedded `_R-` suffix) is not a pure base-10 integer, else `None`.
///
/// Gated on the id's **own leading type marker**: string-typed ids (`_PT-`, `_EN-`,
/// `_CH-`, …) legitimately carry non-numeric suffixes and must not be flagged, even
/// if a readable name happens to embed a substring like `_R-`.
fn bad_integer_suffix(value: &str, prefix: &str) -> Option<String> {
    let scope_start = value.find(prefix)?;
    let after_prefix = &value[scope_start + prefix.len()..];
    // The id's type marker: `_` + chars up to and including the first `-`.
    let marker_end = after_prefix.find('-')? + 1;
    let leading = format!("_{}", &after_prefix[..marker_end]);
    if !INT_MARKERS.contains(&leading.as_str()) {
        return None;
    }
    // Every integer marker occurrence must be a pure integer (a ref id like
    // `<prefix>_UP-1_R-101` carries two).
    for marker in INT_MARKERS {
        let mut search = &value[scope_start..];
        while let Some(pos) = search.find(marker) {
            let after = &search[pos + marker.len()..];
            let seg: String = after
                .chars()
                .take_while(|c| *c != '_' && *c != '"')
                .collect();
            if seg.is_empty() || !seg.chars().all(|c| c.is_ascii_digit()) {
                return Some(format!("{marker}{seg}"));
            }
            search = after;
        }
    }
    None
}

/// Whether an attribute name denotes a reference into the id graph. Covers both the
/// `…RefId` family and the `…RefRef` family (`ParameterRefRef`, `ComObjectRefRef`, and
/// `<Assign>`'s `TargetParamRefRef`/`SourceParamRefRef`), plus `ParamRefId`.
fn is_reference_attr(name: &str) -> bool {
    name.ends_with("RefId") || name.ends_with("RefRef") || name == "ParamRefId"
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::renumber::renumber_ids;

    const PFX: &str = "M-00FA_A-FF01-01-0000";

    fn valid_xml() -> String {
        format!(
            r#"<KNX xmlns="http://knx.org/xml/project/20">
<ManufacturerData><Manufacturer><ApplicationPrograms>
<ApplicationProgram Id="{PFX}">
<Static><Parameters>
<Parameter Id="{PFX}_UP-G000" ParameterType="{PFX}_PT-NumZones" />
</Parameters><ParameterRefs>
<ParameterRef Id="{PFX}_UP-G000_R-{PFX}_UP-G000" RefId="{PFX}_UP-G000" />
</ParameterRefs></Static>
<Dynamic><ChannelIndependentBlock>
<ParameterRefRef RefId="{PFX}_UP-G000_R-{PFX}_UP-G000" />
</ChannelIndependentBlock></Dynamic>
</ApplicationProgram></ApplicationPrograms></Manufacturer></ManufacturerData></KNX>"#
        )
    }

    #[test]
    fn passes_after_renumber() {
        let renumbered = renumber_ids(&valid_xml()).unwrap();
        sanity_check(&renumbered).expect("renumbered xml must pass sanity");
    }

    #[test]
    fn flags_non_integer_suffix() {
        // Raw (un-renumbered) xml has `_UP-G000` → must be rejected.
        let err = sanity_check(&valid_xml()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("non-integer id suffix"), "got: {msg}");
    }

    #[test]
    fn flags_dangling_reference() {
        let xml = format!(
            r#"<KNX><ManufacturerData><Manufacturer><ApplicationPrograms>
<ApplicationProgram Id="{PFX}">
<Static><Parameters>
<Parameter Id="{PFX}_UP-1" />
</Parameters></Static>
<Dynamic><ParameterRefRef RefId="{PFX}_UP-999_R-99901" /></Dynamic>
</ApplicationProgram></ApplicationPrograms></Manufacturer></ManufacturerData></KNX>"#
        );
        let err = sanity_check(&xml).unwrap_err();
        assert!(err.to_string().contains("dangling reference"), "got: {err}");
    }

    #[test]
    fn flags_duplicate_id() {
        let xml = format!(
            r#"<KNX><ManufacturerData><Manufacturer><ApplicationPrograms>
<ApplicationProgram Id="{PFX}">
<Static><Parameters>
<Parameter Id="{PFX}_UP-1" />
<Parameter Id="{PFX}_UP-1" />
</Parameters></Static></ApplicationProgram></ApplicationPrograms></Manufacturer></ManufacturerData></KNX>"#
        );
        let err = sanity_check(&xml).unwrap_err();
        assert!(err.to_string().contains("duplicate Id"), "got: {err}");
    }

    #[test]
    fn flags_dangling_refref_assign() {
        // M2: <Assign> references via *RefRef (not *RefId) must still be validated.
        let xml = format!(
            r#"<KNX><ManufacturerData><Manufacturer><ApplicationPrograms>
<ApplicationProgram Id="{PFX}"><Static><Parameters>
<Parameter Id="{PFX}_UP-1" />
</Parameters></Static>
<Dynamic><Assign TargetParamRefRef="{PFX}_UP-9_R-999" Value="1" /></Dynamic>
</ApplicationProgram></ApplicationPrograms></Manufacturer></ManufacturerData></KNX>"#
        );
        let err = sanity_check(&xml).unwrap_err();
        assert!(err.to_string().contains("dangling reference"), "got: {err}");
    }

    #[test]
    fn string_typed_pt_id_with_embedded_r_not_flagged() {
        // S3: a ParameterType id whose readable name embeds `_R-` is string-typed and
        // must not be rejected as a non-integer suffix.
        let xml = format!(
            r#"<KNX><ManufacturerData><Manufacturer><ApplicationPrograms>
<ApplicationProgram Id="{PFX}"><Static><ParameterTypes>
<ParameterType Id="{PFX}_PT-Reset_R-Delay" Name="x" />
</ParameterTypes></Static></ApplicationProgram></ApplicationPrograms></Manufacturer></ManufacturerData></KNX>"#
        );
        sanity_check(&xml).expect("string _PT- id must not be flagged");
    }
}
