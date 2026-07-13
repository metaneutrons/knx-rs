// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! The [`knxprod!`] declarative DSL for populating an [`AppProgram`](crate::author::AppProgram).
//!
//! The builder API is precise but verbose: every parameter and group object hands
//! back a handle you must bind to a local and thread — sometimes far — down into the
//! `<Dynamic>` tree, where each node is a `Dyn::… { … }` struct literal with `.into()`
//! on every string. `knxprod!` keeps that structure but removes the ceremony: you
//! *name* each type / parameter / object once, and reference it by that name in the
//! tree. Because the names are ordinary Rust bindings, a mistyped reference is a
//! **compile error** (`cannot find value …`), caught before a single byte is emitted —
//! stronger than the build-time panic the imperative path gives.
//!
//! It expands to exactly the builder calls you would otherwise write, so its output is
//! byte-for-byte identical (there is a test asserting precisely that).
//!
//! # Scope
//!
//! `knxprod!` populates the application-program *content*: parameter types,
//! parameters, com-objects, and the `<Dynamic>` tree. It returns the populated
//! [`AppProgram`](crate::author::AppProgram); you attach the document envelope
//! (`ProgramInfo`, catalog, hardware) and emit it with the normal API. It is aimed at
//! products written out literally — loop-generated products (per-zone/-client fan-out)
//! stay in imperative code, which is what loops are for.
//!
//! # Grammar
//!
//! ```text
//! knxprod! {
//!     application <id-expr>;
//!     <item>*
//! }
//! ```
//!
//! where each `<item>` is one of:
//!
//! ```text
//! // A <ParameterType>, bound to `name` (a Rust ident) for later `param`s to reference:
//! type <name> = text   <ets-name>, bits <n>;
//! type <name> = number <ets-name>, bits <n>, min <lo>, max <hi>;          // unsignedInt
//! type <name> = enum   <ets-name>, bits <n>, values { <text> => <val>, … };
//!
//! // A <Parameter>, bound to `name` (its ParameterRef handle); `ty` is a `type` ident:
//! param <name> = <suffix>, <param-name>, <ty>, <text>, <value>, <offset>;
//!
//! // A <ComObject>, bound to `name` (its ComObjectRef handle):
//! object <name> = <suffix>, <name>, <number>, <text>, <function>, <size>,
//!                 dpt (<main>, <sub>), flags [<read|write|transmit|update> …];
//!
//! // The <Dynamic> tree (see the node grammar below):
//! dynamic { <node>* }
//! ```
//!
//! and each `<node>` in a `dynamic { … }` (or a `block`/`cib`/`when` body) is:
//!
//! ```text
//! cib { <node>* }                                   // <ChannelIndependentBlock>
//! channel <suffix>, <number>, <name>, <text> => { <node>* }
//! block <suffix>, <name>, <text> => { <node>* }     // a plain <ParameterBlock>
//! channel_block <suffix>, <name>, <text>, tracks <param> => { <node>* }
//!                                                   // …with TextParameterRefId + ShowInComObjectTree
//! pref <param>;                                      // <ParameterRefRef>
//! oref <object>;                                     // <ComObjectRefRef>
//! sep <suffix>, <text>;                              // <ParameterSeparator UIHint="Headline">
//! sep <suffix>, <text>, hint <hint>;                // …with an explicit UIHint
//! choose <param> { when <test> => { <node>* } … }   // <choose>/<when>
//! ```
//!
//! # Example
//!
//! ```
//! use knx_rs_prod::knxprod;
//!
//! let app = knxprod! {
//!     application "M-00FA_A-FF01-01-0000";
//!
//!     type num_zones = number "NumZones", bits 8, min 1, max 10;
//!     type percent   = number "Percent",  bits 8, min 0, max 100;
//!
//!     param zones = "G000", "G_NumZones", num_zones, "Anzahl Zonen", "10", 0;
//!     param vol   = "Z01002", "Z01_DefVol", percent, "Lautstärke", "50", 1;
//!
//!     object play = "Z01000", "Zone 1 Play", 1, "Play", "Play", "1 Bit",
//!                   dpt (1, 1), flags [read write transmit];
//!
//!     dynamic {
//!         cib {
//!             block "General", "General", "Allgemein" => {
//!                 pref zones;
//!                 choose zones {
//!                     when ">=1" => {
//!                         sep "Z01-Play", "Wiedergabe";
//!                         pref vol;
//!                         oref play;
//!                     }
//!                 }
//!             }
//!         }
//!     }
//! };
//!
//! let mut xml = String::new();
//! app.write_parameters(12, &mut xml);
//! assert!(xml.contains(r#"ParameterType="M-00FA_A-FF01-01-0000_PT-Percent""#));
//! ```
//!
//! # Limits
//!
//! * A `param`/`object` binding is its **ref** handle (`ParameterRefId` /
//!   `ComObjectRefId`) — what the tree needs. For the *entity* handle (`ParamId`, for
//!   `translate_param`) use the imperative API.
//! * Types must be declared before the `param`s that reference them (they are ordinary
//!   `let`s, in source order).
//! * `number` types are `unsignedInt`; other numeric bases use the builder directly.
//! * Deeply nested trees can hit the `macro_rules!` recursion limit; raise it with
//!   `#![recursion_limit = "…"]` or fall back to the builder for the deep part.

/// Populate an [`AppProgram`](crate::author::AppProgram) declaratively. See the
/// [module documentation](crate::author::dsl) for the full grammar and examples.
#[macro_export]
macro_rules! knxprod {
    // ── Entry point ─────────────────────────────────────────────────────────
    (application $id:expr ; $($items:tt)*) => {{
        let mut __app = $crate::author::AppProgram::new($id);
        $crate::knxprod!(@items __app $($items)*);
        __app
    }};

    // ── Items (statement muncher; bindings stay in the block scope) ──────────
    (@items $app:ident) => {};

    (@items $app:ident type $bind:ident = text $ename:expr, bits $bits:expr ; $($rest:tt)*) => {
        let $bind = $app.add_parameter_type($crate::author::ParameterType::text($ename, $bits));
        $crate::knxprod!(@items $app $($rest)*);
    };
    (@items $app:ident type $bind:ident = number $ename:expr, bits $bits:expr, min $min:expr, max $max:expr ; $($rest:tt)*) => {
        let $bind = $app.add_parameter_type(
            $crate::author::ParameterType::number($ename, $bits, "unsignedInt", $min, $max),
        );
        $crate::knxprod!(@items $app $($rest)*);
    };
    (@items $app:ident type $bind:ident = enum $ename:expr, bits $bits:expr, values { $($etext:expr => $eval:expr),* $(,)? } ; $($rest:tt)*) => {
        let $bind = $app.add_parameter_type($crate::author::ParameterType::enumeration(
            $ename,
            $bits,
            ::std::vec![$( (::std::string::String::from($etext), $eval) ),*],
        ));
        $crate::knxprod!(@items $app $($rest)*);
    };

    (@items $app:ident param $bind:ident = $suffix:expr, $pname:expr, $ty:ident, $text:expr, $value:expr, $offset:expr ; $($rest:tt)*) => {
        let $bind = $app
            .add_param($crate::author::Parameter::new($suffix, $pname, $ty, $text, $value, $offset))
            .1;
        $crate::knxprod!(@items $app $($rest)*);
    };

    (@items $app:ident object $bind:ident = $suffix:expr, $oname:expr, $number:expr, $text:expr, $ftext:expr, $size:expr, dpt ( $main:expr, $sub:expr ), flags [ $($flag:ident)* ] ; $($rest:tt)*) => {
        let $bind = $app
            .add_com_object($crate::author::ComObject::new(
                $suffix,
                $oname,
                $number,
                $text,
                $ftext,
                $size,
                $crate::author::Dpt::new($main, $sub),
                {
                    let mut __flags = $crate::author::Flags::default();
                    $( __flags.$flag = true; )*
                    __flags
                },
            ))
            .1;
        $crate::knxprod!(@items $app $($rest)*);
    };

    (@items $app:ident dynamic { $($tree:tt)* } $($rest:tt)*) => {
        for __node in $crate::knxprod!(@nodes [] $($tree)*) {
            $app.add_dynamic(__node);
        }
        $crate::knxprod!(@items $app $($rest)*);
    };

    // ── Dynamic-tree nodes (expression muncher → Vec<Dyn>) ───────────────────
    (@nodes [$($acc:expr,)*]) => { ::std::vec![$($acc,)*] };

    (@nodes [$($acc:expr,)*] pref $h:ident ; $($rest:tt)*) => {
        $crate::knxprod!(@nodes [$($acc,)* $crate::author::Dyn::ParamRefRef($h),] $($rest)*)
    };
    (@nodes [$($acc:expr,)*] oref $h:ident ; $($rest:tt)*) => {
        $crate::knxprod!(@nodes [$($acc,)* $crate::author::Dyn::ComObjRefRef($h),] $($rest)*)
    };
    (@nodes [$($acc:expr,)*] sep $s:expr, $t:expr ; $($rest:tt)*) => {
        $crate::knxprod!(@nodes [$($acc,)* $crate::author::Dyn::Separator {
            suffix: ($s).into(), text: ($t).into(), ui_hint: "Headline".into(),
        },] $($rest)*)
    };
    (@nodes [$($acc:expr,)*] sep $s:expr, $t:expr, hint $hint:expr ; $($rest:tt)*) => {
        $crate::knxprod!(@nodes [$($acc,)* $crate::author::Dyn::Separator {
            suffix: ($s).into(), text: ($t).into(), ui_hint: ($hint).into(),
        },] $($rest)*)
    };
    (@nodes [$($acc:expr,)*] cib { $($c:tt)* } $($rest:tt)*) => {
        $crate::knxprod!(@nodes [$($acc,)*
            $crate::author::Dyn::ChannelIndependentBlock($crate::knxprod!(@nodes [] $($c)*)),
        ] $($rest)*)
    };
    (@nodes [$($acc:expr,)*] channel $s:expr, $num:expr, $n:expr, $t:expr => { $($c:tt)* } $($rest:tt)*) => {
        $crate::knxprod!(@nodes [$($acc,)* $crate::author::Dyn::Channel {
            suffix: ($s).into(), number: ($num).into(), name: ($n).into(), text: ($t).into(),
            children: $crate::knxprod!(@nodes [] $($c)*),
        },] $($rest)*)
    };
    (@nodes [$($acc:expr,)*] block $s:expr, $n:expr, $t:expr => { $($c:tt)* } $($rest:tt)*) => {
        $crate::knxprod!(@nodes [$($acc,)* $crate::author::Dyn::ParameterBlock {
            suffix: ($s).into(), name: ($n).into(), text: ($t).into(),
            text_param_ref: ::std::option::Option::None, show_in_com_object_tree: false,
            children: $crate::knxprod!(@nodes [] $($c)*),
        },] $($rest)*)
    };
    (@nodes [$($acc:expr,)*] channel_block $s:expr, $n:expr, $t:expr, tracks $tp:ident => { $($c:tt)* } $($rest:tt)*) => {
        $crate::knxprod!(@nodes [$($acc,)* $crate::author::Dyn::ParameterBlock {
            suffix: ($s).into(), name: ($n).into(), text: ($t).into(),
            text_param_ref: ::std::option::Option::Some($tp), show_in_com_object_tree: true,
            children: $crate::knxprod!(@nodes [] $($c)*),
        },] $($rest)*)
    };
    (@nodes [$($acc:expr,)*] choose $h:ident { $($w:tt)* } $($rest:tt)*) => {
        $crate::knxprod!(@nodes [$($acc,)* $crate::author::Dyn::Choose {
            param_ref: $h, whens: $crate::knxprod!(@whens [] $($w)*),
        },] $($rest)*)
    };

    // ── choose { when … } branches (expression muncher → Vec<When>) ──────────
    (@whens [$($wacc:expr,)*]) => { ::std::vec![$($wacc,)*] };
    (@whens [$($wacc:expr,)*] when $test:expr => { $($wc:tt)* } $($wrest:tt)*) => {
        $crate::knxprod!(@whens [$($wacc,)* $crate::author::When {
            test: ($test).into(), children: $crate::knxprod!(@nodes [] $($wc)*),
        },] $($wrest)*)
    };
}

#[cfg(test)]
mod tests {
    use crate::author::{AppProgram, ComObject, Dpt, Dyn, Flags, Parameter, ParameterType, When};

    /// The DSL must expand to *exactly* the imperative builder calls — same bytes out.
    #[test]
    fn dsl_output_is_byte_identical_to_the_builder() {
        // ── via the DSL ──────────────────────────────────────────────────
        let dsl = knxprod! {
            application "M-00FA_A-FF01-01-0000";

            type num_zones = number "NumZones", bits 8, min 1, max 10;
            type percent   = number "Percent",  bits 8, min 0, max 100;
            type yesno     = enum   "YesNo",    bits 8, values { "Nein" => 0, "Ja" => 1 };
            type name_t    = text   "Name",     bits 160;

            param zones = "G000", "G_NumZones", num_zones, "Anzahl Zonen", "10", 0;
            param vol   = "Z01002", "Z01_DefVol", percent, "Lautstärke", "50", 1;
            param zname = "Z01000", "Z01_Name", name_t, "Zonenname", "Zone 1", 2;
            param air   = "Z01004", "Z01_AirPlay", yesno, "AirPlay", "1", 162;

            object play = "Z01000", "Zone 1 Play", 1, "Play", "Play", "1 Bit",
                          dpt (1, 1), flags [read write transmit];

            dynamic {
                cib {
                    channel_block "Z01", "Zone1", "Zone 1: {{0: ...}}", tracks zname => {
                        pref zname;
                        sep "Z01-Set", "Einstellungen";
                        pref air;
                        choose zones {
                            when ">=1" => {
                                sep "Z01-Play", "Wiedergabe";
                                pref vol;
                                oref play;
                            }
                        }
                    }
                }
            }
        };
        let mut dsl_xml = String::new();
        dsl.write_parameter_types(12, &mut dsl_xml);
        dsl.write_parameters(12, &mut dsl_xml);
        dsl.write_dynamic(10, &mut dsl_xml);

        // ── the same thing, by hand ──────────────────────────────────────
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        let num_zones =
            app.add_parameter_type(ParameterType::number("NumZones", 8, "unsignedInt", 1, 10));
        let percent =
            app.add_parameter_type(ParameterType::number("Percent", 8, "unsignedInt", 0, 100));
        let yesno = app.add_parameter_type(ParameterType::enumeration(
            "YesNo",
            8,
            vec![("Nein".into(), 0), ("Ja".into(), 1)],
        ));
        let name_t = app.add_parameter_type(ParameterType::text("Name", 160));
        let zones = app
            .add_param(Parameter::new(
                "G000",
                "G_NumZones",
                num_zones,
                "Anzahl Zonen",
                "10",
                0,
            ))
            .1;
        let vol = app
            .add_param(Parameter::new(
                "Z01002",
                "Z01_DefVol",
                percent,
                "Lautstärke",
                "50",
                1,
            ))
            .1;
        let zname = app
            .add_param(Parameter::new(
                "Z01000",
                "Z01_Name",
                name_t,
                "Zonenname",
                "Zone 1",
                2,
            ))
            .1;
        let air = app
            .add_param(Parameter::new(
                "Z01004",
                "Z01_AirPlay",
                yesno,
                "AirPlay",
                "1",
                162,
            ))
            .1;
        let play = app
            .add_com_object(ComObject::new(
                "Z01000",
                "Zone 1 Play",
                1,
                "Play",
                "Play",
                "1 Bit",
                Dpt::new(1, 1),
                Flags {
                    read: true,
                    write: true,
                    transmit: true,
                    update: false,
                },
            ))
            .1;
        app.add_dynamic(Dyn::ChannelIndependentBlock(vec![Dyn::ParameterBlock {
            suffix: "Z01".into(),
            name: "Zone1".into(),
            text: "Zone 1: {{0: ...}}".into(),
            text_param_ref: Some(zname),
            show_in_com_object_tree: true,
            children: vec![
                Dyn::ParamRefRef(zname),
                Dyn::Separator {
                    suffix: "Z01-Set".into(),
                    text: "Einstellungen".into(),
                    ui_hint: "Headline".into(),
                },
                Dyn::ParamRefRef(air),
                Dyn::Choose {
                    param_ref: zones,
                    whens: vec![When {
                        test: ">=1".into(),
                        children: vec![
                            Dyn::Separator {
                                suffix: "Z01-Play".into(),
                                text: "Wiedergabe".into(),
                                ui_hint: "Headline".into(),
                            },
                            Dyn::ParamRefRef(vol),
                            Dyn::ComObjRefRef(play),
                        ],
                    }],
                },
            ],
        }]));
        let mut hand_xml = String::new();
        app.write_parameter_types(12, &mut hand_xml);
        app.write_parameters(12, &mut hand_xml);
        app.write_dynamic(10, &mut hand_xml);

        assert_eq!(dsl_xml, hand_xml);
    }

    #[test]
    fn empty_program_is_valid() {
        let app = knxprod! { application "M-00FA_A-FF01-01-0000"; };
        let mut out = String::new();
        app.write_dynamic(10, &mut out);
        assert!(out.is_empty());
    }
}
