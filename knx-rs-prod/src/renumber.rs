// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Renumber `ApplicationProgram` id suffixes to pure integers.
//!
//! ETS parses the suffix after `_P-`, `_UP-`, `_O-`, `_R-`, `_PB-` and `_PS-`
//! as a base-10 integer **at import time** — a *runtime* rule the `project/NN`
//! XSD does not encode. Product XML authored with readable string suffixes
//! (e.g. snapdog's `_UP-Z01002`, `_O-Z01000`, `_PB-General`) imports fine
//! against the schema but then dies with `'G' is not a legal digit for base
//! 10`. This pass is the pure-Rust equivalent of `OpenKNXproducer`'s
//! `Renumber`/`ConvertKoIds` steps.
//!
//! [`renumber_ids`] assigns each `ApplicationProgram`-scoped id a unique integer
//! suffix and rewrites **every** reference to it (`RefId`, `RefRefId`,
//! `ParamRefId`, …) in lock-step so the reference graph stays consistent.
//!
//! # Scope
//!
//! Only ids under the `ApplicationProgram` id prefix (e.g.
//! `M-00FA_A-FF01-01-0000_…`) are touched. `Hardware`/`Product`/`Catalog` ids
//! (`M-00FA_H-…`) and string-suffix id types ETS accepts verbatim
//! (`_PT-` `ParameterType`, `_EN-` `Enumeration` whose suffix is already the
//! numeric `Value`) are left byte-for-byte unchanged.
//!
//! # How the rewrite stays exact
//!
//! Every id reference in a KNX product XML is a **complete, quoted attribute value**
//! (`Id="…"`, `RefId="…"`, `ParamRefId="…"`, …). [`renumber_ids`] makes a **single
//! tokenizing pass** (`rewrite_ids`): it walks the document once and, for each
//! attribute value that exactly matches a remapped id, emits the new value into a
//! fresh buffer; every other byte (text, comments, non-matching attributes) is copied
//! verbatim and never re-examined. Because no emitted byte is scanned again, the
//! rewrite is exact and order-independent *even when a generated id equals some other
//! old id* (an id permutation) — a case a chained `String::replace` would corrupt.

use std::collections::HashMap;
use std::collections::HashSet;

use crate::error::KnxprodError;
use crate::parse;
use crate::xml_scan;

/// Rewrite every `ApplicationProgram`-scoped id suffix in `xml` to a pure
/// integer and remap all references accordingly.
///
/// Returns the rewritten XML. Formatting and every out-of-scope byte are
/// preserved; only in-scope quoted id values change.
///
/// # Errors
///
/// Returns [`KnxprodError::MissingElement`] / [`KnxprodError::Xml`] if the
/// `ApplicationProgram` id cannot be extracted, or [`KnxprodError::InvalidStructure`]
/// if a reference names an undeclared parent or a `ComObject`/reference carries a
/// non-integer key that cannot be normalised.
pub fn renumber_ids(xml: &str) -> Result<String, KnxprodError> {
    let app_id = parse::extract_application_id(xml)?;
    let map = build_map(xml, &app_id)?;
    Ok(rewrite_ids(xml, &map))
}

/// Rewrite the document in a single tokenizing pass: copy every byte verbatim except
/// attribute values that exactly match a remapped id, which are replaced by their new
/// value. Only open-element attribute values are considered, so ids inside text nodes,
/// comments or CDATA are never touched, and — because output bytes are never
/// re-scanned — an id permutation cannot cascade.
fn rewrite_ids(xml: &str, map: &HashMap<String, String>) -> String {
    let bytes = xml.as_bytes();
    let mut out = String::with_capacity(xml.len() + 64);
    let mut last = 0; // start of the not-yet-flushed verbatim run
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        // Comments / CDATA / closing tags / declarations / PIs are copied verbatim
        // (their ids, if any, are not element attributes we rewrite).
        if xml[i..].starts_with("<!--") {
            i = xml[i..].find("-->").map_or(bytes.len(), |e| i + e + 3);
            continue;
        }
        if xml[i..].starts_with("<![CDATA[") {
            i = xml[i..].find("]]>").map_or(bytes.len(), |e| i + e + 3);
            continue;
        }
        if matches!(bytes.get(i + 1), Some(b'/' | b'!' | b'?')) {
            i = xml_scan::find_tag_end(xml, i).map_or(bytes.len(), |e| e + 1);
            continue;
        }
        let Some(end) = xml_scan::find_tag_end(xml, i) else {
            break;
        };
        // Flush everything before this tag, then emit the tag with values remapped.
        out.push_str(&xml[last..i]);
        rewrite_tag_attrs(&xml[i..=end], map, &mut out);
        i = end + 1;
        last = i;
    }
    out.push_str(&xml[last..]);
    out
}

/// Append `tag` (a full `<…>` open tag) to `out`, replacing any attribute value that
/// is a key in `map`. Quote characters and all other bytes are preserved exactly.
fn rewrite_tag_attrs(tag: &str, map: &HashMap<String, String>, out: &mut String) {
    let b = tag.as_bytes();
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' {
            let val_start = i + 1;
            let mut j = val_start;
            while j < b.len() && b[j] != c {
                j += 1;
            }
            // Flush up to and including the opening quote, then the (maybe remapped)
            // value; the closing quote stays in the next verbatim run.
            out.push_str(&tag[last..val_start]);
            let value = &tag[val_start..j.min(b.len())];
            out.push_str(map.get(value).map_or(value, String::as_str));
            last = j;
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out.push_str(&tag[last..]);
}

/// Look up an attribute value in a parsed attribute list.
fn get_attr<'a>(attrs: &[(&'a str, &'a str)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}

/// Split a reference id at the `_R-` boundary whose left side is a *declared* parent
/// id (present in `map`), returning `(parent, rsuffix)`. This avoids mis-splitting a
/// readable parent suffix that itself contains `_R-`.
fn split_ref<'a>(id: &'a str, map: &HashMap<String, String>) -> Option<(&'a str, &'a str)> {
    let mut from = 0;
    while let Some(rel) = id[from..].find("_R-") {
        let pos = from + rel;
        if map.contains_key(&id[..pos]) {
            return Some((&id[..pos], &id[pos + 3..]));
        }
        from = pos + 3;
    }
    None
}

/// Build the full old-id → new-id map (parents in pass 1, references in pass 2).
#[allow(clippy::too_many_lines)]
fn build_map(xml: &str, app_id: &str) -> Result<HashMap<String, String>, KnxprodError> {
    let prefix = format!("{app_id}_");
    let tags = xml_scan::open_tags(xml);
    let parsed: Vec<(&str, Vec<(&str, &str)>)> = tags
        .iter()
        .map(|t| (t.tag, xml_scan::parse_attrs(t.body)))
        .collect();

    let mut map: HashMap<String, String> = HashMap::new();
    // Parent integer per old parent id, needed to build `_R-` suffixes.
    let mut parent_int: HashMap<String, u64> = HashMap::new();

    // Pass 1 — parameters (P/UP share one namespace), com-objects, blocks,
    // separators. Numbering follows document order for stable, diffable output.
    let mut next_param: u64 = 1;
    let mut next_block: u64 = 1;
    let mut next_separator: u64 = 1;

    for (tag, attrs) in &parsed {
        let Some(id) = get_attr(attrs, "Id") else {
            continue;
        };
        if !id.starts_with(&prefix) {
            continue;
        }
        let suffix = &id[prefix.len()..];
        match *tag {
            "Parameter" => {
                let Some(kind) = param_kind(suffix) else {
                    continue;
                };
                let n = next_param;
                next_param += 1;
                map.insert(id.to_string(), format!("{prefix}{kind}-{n}"));
                parent_int.insert(id.to_string(), n);
            }
            "ComObject" => {
                if !suffix.starts_with("O-") {
                    continue;
                }
                // ETS convention: the ComObject id suffix equals its Number. The Number
                // must itself be a base-10 integer, else the new id is non-integer too.
                let number = get_attr(attrs, "Number").ok_or_else(|| {
                    KnxprodError::InvalidStructure(format!("ComObject {id} has no Number"))
                })?;
                let n: u64 = number.parse().map_err(|_| {
                    KnxprodError::InvalidStructure(format!(
                        "ComObject {id} has non-integer Number {number:?}"
                    ))
                })?;
                map.insert(id.to_string(), format!("{prefix}O-{n}"));
                parent_int.insert(id.to_string(), n);
            }
            "ParameterBlock" => {
                if !suffix.starts_with("PB-") {
                    continue;
                }
                let n = next_block;
                next_block += 1;
                map.insert(id.to_string(), format!("{prefix}PB-{n}"));
            }
            "ParameterSeparator" => {
                if !suffix.starts_with("PS-") {
                    continue;
                }
                let n = next_separator;
                next_separator += 1;
                map.insert(id.to_string(), format!("{prefix}PS-{n}"));
            }
            _ => {}
        }
    }

    // Pass 2a — collect the integer `_R-` suffixes ComObjectRefs already carry, per
    // parent, so any suffix we assign to a *non-integer* sibling can't collide with a
    // kept one.
    let mut used_coref: HashMap<String, HashSet<u64>> = HashMap::new();
    for (tag, attrs) in &parsed {
        if *tag != "ComObjectRef" {
            continue;
        }
        let Some(id) = get_attr(attrs, "Id") else {
            continue;
        };
        if !id.starts_with(&prefix) {
            continue;
        }
        if let Some((parent, rsuffix)) = split_ref(id, &map) {
            if let Ok(n) = rsuffix.parse::<u64>() {
                used_coref.entry(parent.to_string()).or_default().insert(n);
            }
        }
    }

    // Pass 2 — references. A ref id is `<parentId>_R-<suffix>`; the new id is
    // `<newParentId>_R-<newSuffix>`. ParameterRef suffixes are rebuilt from the parent
    // integer (`<parentInt><NN>`, 2-digit per-parent instance, mirroring ETS/OKP);
    // ComObjectRef suffixes are kept if already integer, else assigned the next free
    // per-parent integer.
    let mut pref_inst: HashMap<String, u64> = HashMap::new();
    let mut coref_next: HashMap<String, u64> = HashMap::new();

    for (tag, attrs) in &parsed {
        if !matches!(*tag, "ParameterRef" | "ComObjectRef") {
            continue;
        }
        let Some(id) = get_attr(attrs, "Id") else {
            continue;
        };
        if !id.starts_with(&prefix) {
            continue;
        }
        let (parent, rsuffix) = split_ref(id, &map).ok_or_else(|| {
            KnxprodError::InvalidStructure(format!(
                "{tag} {id} references an undeclared parent (no `_R-` boundary matches a declared id)"
            ))
        })?;
        let new_parent = map[parent].clone();

        let new_rsuffix = match *tag {
            "ParameterRef" => {
                let base = parent_int.get(parent).copied().ok_or_else(|| {
                    KnxprodError::InvalidStructure(format!("ParameterRef {id} parent has no index"))
                })?;
                let inst = pref_inst.entry(parent.to_string()).or_insert(0);
                *inst += 1;
                format!("{base}{inst:02}")
            }
            // ComObjectRef: keep an already-integer suffix, else assign the next
            // per-parent integer not already taken by a kept suffix.
            _ => rsuffix.parse::<u64>().map_or_else(
                |_| {
                    let used = used_coref.entry(parent.to_string()).or_default();
                    let ctr = coref_next.entry(parent.to_string()).or_insert(0);
                    loop {
                        *ctr += 1;
                        if !used.contains(ctr) {
                            break;
                        }
                    }
                    used.insert(*ctr);
                    ctr.to_string()
                },
                |n| n.to_string(),
            ),
        };
        map.insert(id.to_string(), format!("{new_parent}_R-{new_rsuffix}"));
    }

    Ok(map)
}

/// Classify a `_P-`/`_UP-` parameter suffix, returning the id kind (`"P"` or
/// `"UP"`) or `None` if it is neither.
fn param_kind(suffix: &str) -> Option<&'static str> {
    if suffix.starts_with("UP-") {
        Some("UP")
    } else if suffix.starts_with("P-") {
        Some("P")
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const PFX: &str = "M-00FA_A-FF01-01-0000";

    fn sample() -> String {
        format!(
            r#"<KNX xmlns="http://knx.org/xml/project/20">
<ManufacturerData><Manufacturer RefId="M-00FA">
<Hardware><Hardware><Products>
<Product Id="M-00FA_H-0xFF01-1_P-0xFF01" OrderNumber="0xFF01"/>
</Products></Hardware></Hardware>
<ApplicationPrograms><ApplicationProgram Id="{PFX}">
<Static>
<Parameters>
<Parameter Id="{PFX}_UP-G000" Name="G_NumZones" ParameterType="{PFX}_PT-NumZones" Value="10" />
<Parameter Id="{PFX}_UP-Z01000" Name="Z1Name" ParameterType="{PFX}_PT-Text" Value="Zone 1" />
</Parameters>
<ParameterRefs>
<ParameterRef Id="{PFX}_UP-G000_R-{PFX}_UP-G000" RefId="{PFX}_UP-G000" />
<ParameterRef Id="{PFX}_UP-Z01000_R-{PFX}_UP-Z01000" RefId="{PFX}_UP-Z01000" />
</ParameterRefs>
<ComObjectTable>
<ComObject Id="{PFX}_O-Z01000" Name="Zone 1 Play" Number="1" ObjectSize="1 Bit" />
<ComObject Id="{PFX}_O-Z01001" Name="Zone 1 Pause" Number="2" ObjectSize="1 Bit" />
</ComObjectTable>
<ComObjectRefs>
<ComObjectRef Id="{PFX}_O-Z01000_R-1" RefId="{PFX}_O-Z01000" />
<ComObjectRef Id="{PFX}_O-Z01001_R-2" RefId="{PFX}_O-Z01001" />
</ComObjectRefs>
</Static>
<Dynamic><ChannelIndependentBlock>
<ParameterBlock Id="{PFX}_PB-General" Name="Allgemein">
<ParameterSeparator Id="{PFX}_PS-Z01-Playback" Text="Playback" />
<ParameterRefRef RefId="{PFX}_UP-G000_R-{PFX}_UP-G000" />
<choose ParamRefId="{PFX}_UP-G000_R-{PFX}_UP-G000">
<when test="&gt;=1">
<ComObjectRefRef RefId="{PFX}_O-Z01000_R-1" />
</when>
</choose>
</ParameterBlock>
</ChannelIndependentBlock></Dynamic>
</ApplicationProgram></ApplicationPrograms>
</Manufacturer></ManufacturerData></KNX>"#
        )
    }

    #[test]
    fn all_inscope_suffixes_become_integers() {
        let out = renumber_ids(&sample()).unwrap();
        // No *in-scope* id keeps a non-numeric suffix after its type marker.
        // Out-of-scope Hardware ids (e.g. `M-00FA_H-…_P-0xFF01`) are ignored.
        let app_pfx = format!("{PFX}_");
        for marker in ["_UP-", "_P-", "_O-", "_R-", "_PB-", "_PS-"] {
            for (i, _) in out.match_indices(marker) {
                // Only check markers belonging to an id under the app prefix:
                // walk back to the opening quote and require the app prefix.
                let before = &out[..i];
                let Some(q) = before.rfind('"') else { continue };
                if !out[q + 1..].starts_with(&app_pfx) {
                    continue;
                }
                let rest = &out[i + marker.len()..];
                let seg: String = rest
                    .chars()
                    .take_while(|c| *c != '_' && *c != '"')
                    .collect();
                assert!(
                    seg.chars().all(|c| c.is_ascii_digit()) && !seg.is_empty(),
                    "non-integer suffix after {marker}: {seg:?}"
                );
            }
        }
    }

    #[test]
    fn references_stay_consistent() {
        let out = renumber_ids(&sample()).unwrap();
        // The ParameterRefRef / choose still point at the renumbered ref id.
        // G000 -> UP-1, its ref -> _R-101 (parent int 1, instance 01).
        assert!(out.contains(r#"<ParameterRef Id="M-00FA_A-FF01-01-0000_UP-1_R-101" RefId="M-00FA_A-FF01-01-0000_UP-1" />"#));
        assert!(out.contains(r#"<ParameterRefRef RefId="M-00FA_A-FF01-01-0000_UP-1_R-101" />"#));
        assert!(out.contains(r#"<choose ParamRefId="M-00FA_A-FF01-01-0000_UP-1_R-101">"#));
        // ComObject id suffix == Number; its ref keeps the integer suffix.
        assert!(out.contains(
            r#"<ComObject Id="M-00FA_A-FF01-01-0000_O-1" Name="Zone 1 Play" Number="1""#
        ));
        assert!(out.contains(r#"<ComObjectRef Id="M-00FA_A-FF01-01-0000_O-1_R-1" RefId="M-00FA_A-FF01-01-0000_O-1" />"#));
        assert!(out.contains(r#"<ComObjectRefRef RefId="M-00FA_A-FF01-01-0000_O-1_R-1" />"#));
    }

    #[test]
    fn out_of_scope_ids_untouched() {
        let out = renumber_ids(&sample()).unwrap();
        // Hardware/Product ids and string-typed ParameterType/Enumeration stay.
        assert!(out.contains(r#"<Product Id="M-00FA_H-0xFF01-1_P-0xFF01" OrderNumber="0xFF01"/>"#));
        assert!(out.contains(r#"ParameterType="M-00FA_A-FF01-01-0000_PT-NumZones""#));
    }

    #[test]
    fn idempotent_on_integer_ids() {
        let once = renumber_ids(&sample()).unwrap();
        let twice = renumber_ids(&once).unwrap();
        assert_eq!(once, twice, "renumber must be idempotent");
    }

    /// Wrap `Static`-section snippets in a minimal `ApplicationProgram` document.
    fn doc(inner: &str) -> String {
        format!(
            r#"<KNX xmlns="http://knx.org/xml/project/20"><ManufacturerData><Manufacturer>
<ApplicationPrograms><ApplicationProgram Id="{PFX}"><Static>{inner}</Static>
</ApplicationProgram></ApplicationPrograms></Manufacturer></ManufacturerData></KNX>"#
        )
    }

    #[test]
    fn id_permutation_does_not_corrupt() {
        // Integer suffixes in REVERSE document order → the map is a swap. A chained
        // String::replace collapses both to one id; the single-pass rewrite must not.
        let out = renumber_ids(&doc(&format!(
            r#"<Parameters>
<Parameter Id="{PFX}_UP-2" Name="B" ParameterType="{PFX}_PT-x" />
<Parameter Id="{PFX}_UP-1" Name="A" ParameterType="{PFX}_PT-x" />
</Parameters>"#
        )))
        .unwrap();
        assert!(out.contains(r#"<Parameter Id="M-00FA_A-FF01-01-0000_UP-1" Name="B""#));
        assert!(out.contains(r#"<Parameter Id="M-00FA_A-FF01-01-0000_UP-2" Name="A""#));
        assert_eq!(out.matches(r#"Id="M-00FA_A-FF01-01-0000_UP-1""#).count(), 1);
        assert_eq!(out.matches(r#"Id="M-00FA_A-FF01-01-0000_UP-2""#).count(), 1);
    }

    #[test]
    fn comobject_non_integer_number_errors() {
        let err = renumber_ids(&doc(&format!(
            r#"<ComObjectTable><ComObject Id="{PFX}_O-Zx" Number="0x1F" ObjectSize="1 Bit" /></ComObjectTable>"#
        )))
        .unwrap_err();
        assert!(
            matches!(err, KnxprodError::InvalidStructure(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn undeclared_parent_errors() {
        let err = renumber_ids(&doc(&format!(
            r#"<ParameterRefs><ParameterRef Id="{PFX}_UP-9_R-x" RefId="{PFX}_UP-9" /></ParameterRefs>"#
        )))
        .unwrap_err();
        assert!(
            matches!(err, KnxprodError::InvalidStructure(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn ref_boundary_uses_declared_parent() {
        // Parent suffix itself contains `_R-`; split_once would mis-split.
        let out = renumber_ids(&doc(&format!(
            r#"<Parameters>
<Parameter Id="{PFX}_UP-Level_R-Hi" ParameterType="{PFX}_PT-x" />
</Parameters><ParameterRefs>
<ParameterRef Id="{PFX}_UP-Level_R-Hi_R-{PFX}_UP-Level_R-Hi" RefId="{PFX}_UP-Level_R-Hi" />
</ParameterRefs>"#
        )))
        .unwrap();
        assert!(out.contains(r#"<Parameter Id="M-00FA_A-FF01-01-0000_UP-1""#));
        assert!(out.contains(r#"<ParameterRef Id="M-00FA_A-FF01-01-0000_UP-1_R-101" RefId="M-00FA_A-FF01-01-0000_UP-1""#));
    }

    #[test]
    fn comobjectref_mixed_suffixes_no_collision() {
        // One ComObjectRef keeps integer "1"; a sibling non-integer suffix must be
        // assigned a DIFFERENT free integer (not collide on _R-1).
        let out = renumber_ids(&doc(&format!(
            r#"<ComObjectTable><ComObject Id="{PFX}_O-Zx" Number="5" ObjectSize="1 Bit" /></ComObjectTable>
<ComObjectRefs>
<ComObjectRef Id="{PFX}_O-Zx_R-1" RefId="{PFX}_O-Zx" />
<ComObjectRef Id="{PFX}_O-Zx_R-Foo" RefId="{PFX}_O-Zx" />
</ComObjectRefs>"#
        )))
        .unwrap();
        assert!(out.contains(r#"<ComObjectRef Id="M-00FA_A-FF01-01-0000_O-5_R-1""#));
        // The non-integer sibling got a fresh, distinct integer (not _R-1).
        assert_eq!(
            out.matches(r#"Id="M-00FA_A-FF01-01-0000_O-5_R-1""#).count(),
            1
        );
        assert!(out.contains(r#"<ComObjectRef Id="M-00FA_A-FF01-01-0000_O-5_R-2""#));
    }

    #[test]
    fn gt_in_attribute_value_is_tolerated() {
        let out = renumber_ids(&doc(&format!(
            r#"<Parameters><Parameter Id="{PFX}_UP-A" Name="Level > 50%" ParameterType="{PFX}_PT-x" /></Parameters>"#
        )))
        .unwrap();
        assert!(out.contains(r#"<Parameter Id="M-00FA_A-FF01-01-0000_UP-1" Name="Level > 50%""#));
    }

    #[test]
    fn single_quoted_attribute_ids_rewritten() {
        // App id double-quoted (metadata parse), a param id single-quoted.
        let out = renumber_ids(&doc(&format!(
            "<Parameters><Parameter Id='{PFX}_UP-Z01' Name='n' ParameterType='{PFX}_PT-x' /></Parameters>"
        )))
        .unwrap();
        assert!(
            out.contains("Id='M-00FA_A-FF01-01-0000_UP-1'"),
            "got: {out}"
        );
    }

    #[test]
    fn ids_inside_comments_are_not_rewritten() {
        let out = renumber_ids(&doc(&format!(
            r#"<Parameters>
<!-- see "{PFX}_UP-Z01" -->
<Parameter Id="{PFX}_UP-Z01" ParameterType="{PFX}_PT-x" />
</Parameters>"#
        )))
        .unwrap();
        assert!(out.contains(r#"<!-- see "M-00FA_A-FF01-01-0000_UP-Z01" -->"#));
        assert!(out.contains(r#"<Parameter Id="M-00FA_A-FF01-01-0000_UP-1""#));
    }

    #[test]
    fn multiple_refs_per_parent_get_distinct_suffixes() {
        let out = renumber_ids(&doc(&format!(
            r#"<Parameters><Parameter Id="{PFX}_UP-Z01" ParameterType="{PFX}_PT-x" /></Parameters>
<ParameterRefs>
<ParameterRef Id="{PFX}_UP-Z01_R-a" RefId="{PFX}_UP-Z01" />
<ParameterRef Id="{PFX}_UP-Z01_R-b" RefId="{PFX}_UP-Z01" />
</ParameterRefs>"#
        )))
        .unwrap();
        assert!(out.contains(r#"<ParameterRef Id="M-00FA_A-FF01-01-0000_UP-1_R-101""#));
        assert!(out.contains(r#"<ParameterRef Id="M-00FA_A-FF01-01-0000_UP-1_R-102""#));
    }
}
