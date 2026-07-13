// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! The document envelope — the `<KNX>` / `<ManufacturerData>` / `<Manufacturer>`
//! wrapper and the `<ApplicationProgram>` element that holds every `<Static>`
//! section plus the `<Dynamic>` tree.
//!
//! [`AppProgram::write_knx_document`] renders a whole product XML from a single
//! populated [`AppProgram`], so the entire document — not just its sections — comes
//! from the typed model.

use std::fmt::Write as _;

use super::{AppProgram, escape_attr, write_address_table, write_association_table, xml_bool};

/// The attributes of the `<ApplicationProgram>` element. The conventional ones
/// default to their common ETS values; override them with the builder setters.
#[derive(Clone, Debug)]
pub struct ProgramInfo {
    name: String,
    mask_version: String,
    default_language: String,
    application_number: u32,
    application_version: u32,
    program_type: String,
    load_procedure_style: String,
    pei_type: String,
    dynamic_table_management: bool,
    linkable: bool,
    min_ets_version: String,
    ip_config: String,
}

impl ProgramInfo {
    /// A program identified by `name`, its BAU `mask_version` (e.g. `MV-07B0`),
    /// `default_language`, and the `ApplicationNumber`/`ApplicationVersion` ETS keys
    /// the import on. The remaining attributes default to common ETS values
    /// (`ProgramType="ApplicationProgram"`, `LoadProcedureStyle="MergedProcedure"`,
    /// `PeiType="0"`, `DynamicTableManagement="false"`, `Linkable="true"`,
    /// `MinEtsVersion="5.0"`, `IPConfig="Custom"`).
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        mask_version: impl Into<String>,
        default_language: impl Into<String>,
        application_number: u32,
        application_version: u32,
    ) -> Self {
        Self {
            name: name.into(),
            mask_version: mask_version.into(),
            default_language: default_language.into(),
            application_number,
            application_version,
            program_type: "ApplicationProgram".to_string(),
            load_procedure_style: "MergedProcedure".to_string(),
            pei_type: "0".to_string(),
            dynamic_table_management: false,
            linkable: true,
            min_ets_version: "5.0".to_string(),
            ip_config: "Custom".to_string(),
        }
    }

    /// Override `ProgramType` (default `ApplicationProgram`).
    #[must_use]
    pub fn program_type(mut self, value: impl Into<String>) -> Self {
        self.program_type = value.into();
        self
    }

    /// Override `LoadProcedureStyle` (default `MergedProcedure`).
    #[must_use]
    pub fn load_procedure_style(mut self, value: impl Into<String>) -> Self {
        self.load_procedure_style = value.into();
        self
    }

    /// Override `PeiType` (default `0`).
    #[must_use]
    pub fn pei_type(mut self, value: impl Into<String>) -> Self {
        self.pei_type = value.into();
        self
    }

    /// Override `DynamicTableManagement` (default `false`).
    #[must_use]
    pub const fn dynamic_table_management(mut self, value: bool) -> Self {
        self.dynamic_table_management = value;
        self
    }

    /// Override `Linkable` (default `true`).
    #[must_use]
    pub const fn linkable(mut self, value: bool) -> Self {
        self.linkable = value;
        self
    }

    /// Override `MinEtsVersion` (default `5.0`).
    #[must_use]
    pub fn min_ets_version(mut self, value: impl Into<String>) -> Self {
        self.min_ets_version = value.into();
        self
    }

    /// Override `IPConfig` (default `Custom`).
    #[must_use]
    pub fn ip_config(mut self, value: impl Into<String>) -> Self {
        self.ip_config = value.into();
        self
    }
}

impl AppProgram {
    /// Emit the `<ApplicationPrograms>` / `<ApplicationProgram>` element at `indent`
    /// spaces: the attribute line (with `ReplacesVersions` derived as `0..version`),
    /// the ordered `<Static>` sections, then the `<Dynamic>` tree.
    pub fn write_application_program(&self, info: &ProgramInfo, indent: usize, out: &mut String) {
        let l0 = " ".repeat(indent);
        let l1 = " ".repeat(indent + 2);
        let l2 = " ".repeat(indent + 4);
        // ReplacesVersions lists every prior version so ETS offers an in-place upgrade.
        let replaces = (0..info.application_version)
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let _ = writeln!(out, "{l0}<ApplicationPrograms>");
        let _ = writeln!(
            out,
            r#"{l1}<ApplicationProgram Id="{id}" ProgramType="{pt}" MaskVersion="{mv}" Name="{name}" LoadProcedureStyle="{lps}" PeiType="{pei}" DefaultLanguage="{lang}" DynamicTableManagement="{dtm}" Linkable="{link}" MinEtsVersion="{mev}" IPConfig="{ipc}" ApplicationNumber="{an}" ApplicationVersion="{av}" ReplacesVersions="{rv}">"#,
            id = self.app_prefix,
            pt = escape_attr(&info.program_type),
            mv = escape_attr(&info.mask_version),
            name = escape_attr(&info.name),
            lps = escape_attr(&info.load_procedure_style),
            pei = escape_attr(&info.pei_type),
            lang = escape_attr(&info.default_language),
            dtm = xml_bool(info.dynamic_table_management),
            link = xml_bool(info.linkable),
            mev = escape_attr(&info.min_ets_version),
            ipc = escape_attr(&info.ip_config),
            an = info.application_number,
            av = info.application_version,
            rv = replaces,
        );
        let _ = writeln!(out, "{l2}<Static>");
        let inner = indent + 6;
        self.write_code(inner, out);
        self.write_parameter_types(inner, out);
        self.write_parameters(inner, out);
        self.write_parameter_refs(inner, out);
        self.write_com_object_table(inner, out);
        self.write_com_object_refs(inner, out);
        if let Some(max) = self.address_table_max {
            write_address_table(inner, max, out);
        }
        if let Some(max) = self.association_table_max {
            write_association_table(inner, max, out);
        }
        self.write_load_procedures(inner, out);
        self.write_messages(inner, out);
        if let Some(options) = &self.options {
            options.write(inner, out);
        }
        let _ = writeln!(out, "{l2}</Static>");
        self.write_dynamic(indent + 4, out);
        let _ = writeln!(out, "{l1}</ApplicationProgram>");
        let _ = writeln!(out, "{l0}</ApplicationPrograms>");
    }

    /// Emit a complete `<KNX>` product document: the XML declaration, the
    /// `<ManufacturerData>`/`<Manufacturer>` wrapper, and — in ETS order — the
    /// `<Catalog>`, the application program, and the `<Hardware>`.
    pub fn write_knx_document(
        &self,
        info: &ProgramInfo,
        created_by: &str,
        tool_version: &str,
        out: &mut String,
    ) {
        let _ = writeln!(out, r#"<?xml version="1.0" encoding="utf-8"?>"#);
        let _ = writeln!(
            out,
            r#"<KNX xmlns="http://knx.org/xml/project/20" CreatedBy="{cb}" ToolVersion="{tv}">"#,
            cb = escape_attr(created_by),
            tv = escape_attr(tool_version),
        );
        let _ = writeln!(out, "  <ManufacturerData>");
        let _ = writeln!(
            out,
            r#"    <Manufacturer RefId="{mfr}">"#,
            mfr = self.manufacturer(),
        );
        self.write_catalog(6, out);
        self.write_application_program(info, 6, out);
        self.write_hardware(6, out);
        let _ = writeln!(out, "    </Manufacturer>");
        let _ = writeln!(out, "  </ManufacturerData>");
        let _ = writeln!(out, "</KNX>");
    }
}

#[cfg(test)]
mod tests {
    use super::super::{AppProgram, Options};
    use super::*;

    #[test]
    fn document_envelope_matches_ets_bytes() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        app.set_address_table(2047);
        app.set_association_table(2047);
        app.set_options(Options::new("iso-8859-15", true, true));
        let info = ProgramInfo::new("SnapDog", "MV-07B0", "de-DE", 65281, 8);
        let mut out = String::new();
        app.write_knx_document(&info, "SnapDog xtask", "1.0", &mut out);

        assert!(
            out.starts_with(concat!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
                "<KNX xmlns=\"http://knx.org/xml/project/20\" CreatedBy=\"SnapDog xtask\" ToolVersion=\"1.0\">\n",
                "  <ManufacturerData>\n",
                "    <Manufacturer RefId=\"M-00FA\">\n",
            )),
            "{out}"
        );
        assert!(
            out.contains(concat!(
                "      <ApplicationPrograms>\n",
                "        <ApplicationProgram Id=\"M-00FA_A-FF01-01-0000\" ProgramType=\"ApplicationProgram\" MaskVersion=\"MV-07B0\" Name=\"SnapDog\" LoadProcedureStyle=\"MergedProcedure\" PeiType=\"0\" DefaultLanguage=\"de-DE\" DynamicTableManagement=\"false\" Linkable=\"true\" MinEtsVersion=\"5.0\" IPConfig=\"Custom\" ApplicationNumber=\"65281\" ApplicationVersion=\"8\" ReplacesVersions=\"0 1 2 3 4 5 6 7\">\n",
                "          <Static>\n",
            )),
            "{out}"
        );
        assert!(
            out.contains("            <AddressTable MaxEntries=\"2047\" />\n"),
            "{out}"
        );
        assert!(
            out.contains("            <AssociationTable MaxEntries=\"2047\" />\n"),
            "{out}"
        );
        assert!(
            out.contains("            <Options TextParameterEncoding=\"iso-8859-15\" SupportsExtendedMemoryServices=\"true\" SupportsExtendedPropertyServices=\"true\" />\n"),
            "{out}"
        );
        assert!(
            out.ends_with(concat!(
                "        </ApplicationProgram>\n",
                "      </ApplicationPrograms>\n",
                "    </Manufacturer>\n",
                "  </ManufacturerData>\n",
                "</KNX>\n",
            )),
            "{out}"
        );
    }
}
