// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Static product-identity sections: `<Catalog>`, `<Hardware>`, `<Messages>`,
//! `<Options>`, and the address/association tables.
//!
//! ETS keys a product by hardware identity — a serial plus a version — and threads
//! that identity through a web of ids (`H-<serial>-<version>`, `P-<order>`,
//! `HP-<app>`, `CI-<serial>-<version>`, `CS-<section>`). The [`Hardware`] and
//! [`CatalogSection`] builders take the identifying *components* and derive every
//! id from them, so the cross-references between the catalog and the hardware
//! cannot fall out of sync.

use std::fmt::Write as _;

use super::{AppProgram, escape_attr, xml_bool};

// ── Hardware ────────────────────────────────────────────────────────────────

/// A `<Product>` under a [`Hardware`] — a purchasable order number.
#[derive(Clone, Debug)]
pub struct Product {
    order_number: String,
    text: String,
    is_rail_mounted: bool,
    default_language: String,
}

impl Product {
    /// A product identified by its `order_number` (also its `_P-` id tail).
    #[must_use]
    pub fn new(
        order_number: impl Into<String>,
        text: impl Into<String>,
        default_language: impl Into<String>,
    ) -> Self {
        Self {
            order_number: order_number.into(),
            text: text.into(),
            is_rail_mounted: false,
            default_language: default_language.into(),
        }
    }

    /// Set `IsRailMounted` (default `false`).
    #[must_use]
    pub const fn rail_mounted(mut self, yes: bool) -> Self {
        self.is_rail_mounted = yes;
        self
    }
}

/// A `<Hardware2Program>` linking a [`Hardware`] to its application program.
#[derive(Clone, Debug)]
pub struct Hardware2Program {
    medium_types: String,
    registration_number: String,
}

impl Hardware2Program {
    /// A hardware↔program link with the given `MediumTypes` (e.g. `MT-0`) and
    /// `RegistrationNumber` (`\d{4}/\d+`, what marks the product as registered).
    #[must_use]
    pub fn new(medium_types: impl Into<String>, registration_number: impl Into<String>) -> Self {
        Self {
            medium_types: medium_types.into(),
            registration_number: registration_number.into(),
        }
    }
}

/// A `<Hardware>` entry — one device identity (serial + version) with its products
/// and application-program links.
#[derive(Clone, Debug)]
pub struct Hardware {
    serial_number: String,
    version_number: u32,
    name: String,
    bus_current: u32,
    has_individual_address: bool,
    has_application_program: bool,
    products: Vec<Product>,
    programs: Vec<Hardware2Program>,
}

impl Hardware {
    /// A device identified by `serial_number` + `version_number` (its `_H-` id).
    #[must_use]
    pub fn new(
        serial_number: impl Into<String>,
        version_number: u32,
        name: impl Into<String>,
    ) -> Self {
        Self {
            serial_number: serial_number.into(),
            version_number,
            name: name.into(),
            bus_current: 0,
            has_individual_address: true,
            has_application_program: true,
            products: Vec::new(),
            programs: Vec::new(),
        }
    }

    /// Set `BusCurrent` in mA (default `0`).
    #[must_use]
    pub const fn bus_current(mut self, ma: u32) -> Self {
        self.bus_current = ma;
        self
    }

    /// Set `HasIndividualAddress` (default `true`).
    #[must_use]
    pub const fn has_individual_address(mut self, yes: bool) -> Self {
        self.has_individual_address = yes;
        self
    }

    /// Set `HasApplicationProgram` (default `true`).
    #[must_use]
    pub const fn has_application_program(mut self, yes: bool) -> Self {
        self.has_application_program = yes;
        self
    }

    /// Add a `<Product>`.
    #[must_use]
    pub fn with_product(mut self, product: Product) -> Self {
        self.products.push(product);
        self
    }

    /// Add a `<Hardware2Program>`.
    #[must_use]
    pub fn with_program(mut self, program: Hardware2Program) -> Self {
        self.programs.push(program);
        self
    }
}

// ── Catalog ─────────────────────────────────────────────────────────────────

/// A `<CatalogItem>` — the entry a user picks in the ETS catalog.
///
/// Its `ProductRefId` and `Hardware2ProgramRefId` are derived from the same
/// `serial`/`version`/`order` components as the matching [`Hardware`], so they
/// cannot dangle.
#[derive(Clone, Debug)]
pub struct CatalogItem {
    name: String,
    number: String,
    serial_number: String,
    version_number: u32,
    order_number: String,
    default_language: String,
}

impl CatalogItem {
    /// A catalog entry for the hardware identified by `serial`/`version`/`order`.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        number: impl Into<String>,
        serial_number: impl Into<String>,
        version_number: u32,
        order_number: impl Into<String>,
        default_language: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            number: number.into(),
            serial_number: serial_number.into(),
            version_number,
            order_number: order_number.into(),
            default_language: default_language.into(),
        }
    }
}

/// A `<CatalogSection>` (id `_CS-<key>`) grouping catalog items.
#[derive(Clone, Debug)]
pub struct CatalogSection {
    key: String,
    name: String,
    number: String,
    default_language: String,
    items: Vec<CatalogItem>,
}

impl CatalogSection {
    /// A section whose `_CS-` id tail is `key`.
    #[must_use]
    pub fn new(
        key: impl Into<String>,
        name: impl Into<String>,
        number: impl Into<String>,
        default_language: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            number: number.into(),
            default_language: default_language.into(),
            items: Vec::new(),
        }
    }

    /// Add a `<CatalogItem>`.
    #[must_use]
    pub fn with_item(mut self, item: CatalogItem) -> Self {
        self.items.push(item);
        self
    }
}

// ── Messages ────────────────────────────────────────────────────────────────

/// A `<Message>` (id `_M-<suffix>`) surfaced by a load procedure on error.
#[derive(Clone, Debug)]
pub struct Message {
    suffix: String,
    name: String,
    text: String,
}

impl Message {
    /// A message whose `_M-` id tail is `suffix`.
    #[must_use]
    pub fn new(
        suffix: impl Into<String>,
        name: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            suffix: suffix.into(),
            name: name.into(),
            text: text.into(),
        }
    }
}

// ── Options ─────────────────────────────────────────────────────────────────

/// The `<Options>` element — application-program load/encoding capabilities.
#[derive(Clone, Debug)]
pub struct Options {
    text_parameter_encoding: String,
    supports_extended_memory_services: bool,
    supports_extended_property_services: bool,
}

impl Options {
    /// Options with the given text-parameter `encoding` (e.g. `iso-8859-15`).
    #[must_use]
    pub fn new(
        encoding: impl Into<String>,
        supports_extended_memory_services: bool,
        supports_extended_property_services: bool,
    ) -> Self {
        Self {
            text_parameter_encoding: encoding.into(),
            supports_extended_memory_services,
            supports_extended_property_services,
        }
    }

    /// Emit the `<Options>` element at `indent` spaces.
    pub fn write(&self, indent: usize, out: &mut String) {
        let _ = writeln!(
            out,
            r#"{pad}<Options TextParameterEncoding="{enc}" SupportsExtendedMemoryServices="{mem}" SupportsExtendedPropertyServices="{prop}" />"#,
            pad = " ".repeat(indent),
            enc = escape_attr(&self.text_parameter_encoding),
            mem = xml_bool(self.supports_extended_memory_services),
            prop = xml_bool(self.supports_extended_property_services),
        );
    }
}

/// Emit an `<AddressTable MaxEntries>` at `indent` spaces.
pub fn write_address_table(indent: usize, max_entries: u32, out: &mut String) {
    let _ = writeln!(
        out,
        r#"{pad}<AddressTable MaxEntries="{max_entries}" />"#,
        pad = " ".repeat(indent),
    );
}

/// Emit an `<AssociationTable MaxEntries>` at `indent` spaces.
pub fn write_association_table(indent: usize, max_entries: u32, out: &mut String) {
    let _ = writeln!(
        out,
        r#"{pad}<AssociationTable MaxEntries="{max_entries}" />"#,
        pad = " ".repeat(indent),
    );
}

impl AppProgram {
    /// The application-program id tail after `_A-` (e.g. `FF01-01-0000`), which the
    /// hardware↔program id (`_HP-`) is built from.
    fn app_tail(&self) -> &str {
        self.app_prefix
            .split_once("_A-")
            .map_or("", |(_, tail)| tail)
    }

    /// Register a `<CatalogSection>`; sections are emitted in registration order.
    pub fn add_catalog_section(&mut self, section: CatalogSection) {
        self.catalog_sections.push(section);
    }

    /// Register a `<Hardware>` entry; entries are emitted in registration order.
    pub fn add_hardware(&mut self, hardware: Hardware) {
        self.hardware.push(hardware);
    }

    /// Register a `<Message>`; messages are emitted in registration order.
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Set the `<AddressTable MaxEntries>` emitted in `<Static>`.
    pub const fn set_address_table(&mut self, max_entries: u32) {
        self.address_table_max = Some(max_entries);
    }

    /// Set the `<AssociationTable MaxEntries>` emitted in `<Static>`.
    pub const fn set_association_table(&mut self, max_entries: u32) {
        self.association_table_max = Some(max_entries);
    }

    /// Set the `<Options>` element emitted in `<Static>`.
    pub fn set_options(&mut self, options: Options) {
        self.options = Some(options);
    }

    /// Emit the manufacturer-level `<Catalog>` block at `indent` spaces. Emits
    /// nothing when no sections were registered.
    pub fn write_catalog(&self, indent: usize, out: &mut String) {
        if self.catalog_sections.is_empty() {
            return;
        }
        let mfr = self.manufacturer().to_string();
        let app_tail = self.app_tail().to_string();
        let l0 = " ".repeat(indent);
        let l1 = " ".repeat(indent + 2);
        let l2 = " ".repeat(indent + 4);
        let _ = writeln!(out, "{l0}<Catalog>");
        for section in &self.catalog_sections {
            let _ = writeln!(
                out,
                r#"{l1}<CatalogSection Id="{mfr}_CS-{key}" Name="{name}" Number="{number}" DefaultLanguage="{lang}">"#,
                key = section.key,
                name = escape_attr(&section.name),
                number = escape_attr(&section.number),
                lang = escape_attr(&section.default_language),
            );
            for item in &section.items {
                let hw = format!(
                    "{mfr}_H-{s}-{v}",
                    s = item.serial_number,
                    v = item.version_number
                );
                let _ = writeln!(
                    out,
                    r#"{l2}<CatalogItem Id="{hw}_HP-{app_tail}_CI-{s}-{v}" Name="{name}" Number="{number}" ProductRefId="{hw}_P-{order}" Hardware2ProgramRefId="{hw}_HP-{app_tail}" DefaultLanguage="{lang}" />"#,
                    s = item.serial_number,
                    v = item.version_number,
                    name = escape_attr(&item.name),
                    number = escape_attr(&item.number),
                    order = item.order_number,
                    lang = escape_attr(&item.default_language),
                );
            }
            let _ = writeln!(out, "{l1}</CatalogSection>");
        }
        let _ = writeln!(out, "{l0}</Catalog>");
    }

    /// Emit the manufacturer-level `<Hardware>` block at `indent` spaces. Emits
    /// nothing when no hardware was registered.
    pub fn write_hardware(&self, indent: usize, out: &mut String) {
        if self.hardware.is_empty() {
            return;
        }
        let mfr = self.manufacturer().to_string();
        let app_tail = self.app_tail().to_string();
        let l0 = " ".repeat(indent);
        let l1 = " ".repeat(indent + 2);
        let l2 = " ".repeat(indent + 4);
        let l3 = " ".repeat(indent + 6);
        let l4 = " ".repeat(indent + 8);
        let _ = writeln!(out, "{l0}<Hardware>");
        for hw in &self.hardware {
            let hw_id = format!(
                "{mfr}_H-{s}-{v}",
                s = hw.serial_number,
                v = hw.version_number
            );
            let _ = writeln!(
                out,
                r#"{l1}<Hardware Id="{hw_id}" Name="{name}" SerialNumber="{serial}" VersionNumber="{ver}" BusCurrent="{bus}" HasIndividualAddress="{hia}" HasApplicationProgram="{hap}">"#,
                name = escape_attr(&hw.name),
                serial = escape_attr(&hw.serial_number),
                ver = hw.version_number,
                bus = hw.bus_current,
                hia = xml_bool(hw.has_individual_address),
                hap = xml_bool(hw.has_application_program),
            );
            let _ = writeln!(out, "{l2}<Products>");
            for p in &hw.products {
                let _ = writeln!(
                    out,
                    r#"{l3}<Product Id="{hw_id}_P-{order}" Text="{text}" OrderNumber="{order}" IsRailMounted="{rail}" DefaultLanguage="{lang}">"#,
                    order = p.order_number,
                    text = escape_attr(&p.text),
                    rail = xml_bool(p.is_rail_mounted),
                    lang = escape_attr(&p.default_language),
                );
                let _ = writeln!(
                    out,
                    r#"{l4}<RegistrationInfo RegistrationStatus="Registered" />"#
                );
                let _ = writeln!(out, "{l3}</Product>");
            }
            let _ = writeln!(out, "{l2}</Products>");
            let _ = writeln!(out, "{l2}<Hardware2Programs>");
            for prog in &hw.programs {
                let _ = writeln!(
                    out,
                    r#"{l3}<Hardware2Program Id="{hw_id}_HP-{app_tail}" MediumTypes="{mt}">"#,
                    mt = escape_attr(&prog.medium_types),
                );
                let _ = writeln!(
                    out,
                    r#"{l4}<ApplicationProgramRef RefId="{prefix}" />"#,
                    prefix = self.app_prefix
                );
                let _ = writeln!(
                    out,
                    r#"{l4}<RegistrationInfo RegistrationStatus="Registered" RegistrationNumber="{rn}" />"#,
                    rn = escape_attr(&prog.registration_number),
                );
                let _ = writeln!(out, "{l3}</Hardware2Program>");
            }
            let _ = writeln!(out, "{l2}</Hardware2Programs>");
            let _ = writeln!(out, "{l1}</Hardware>");
        }
        let _ = writeln!(out, "{l0}</Hardware>");
    }

    /// Emit the `<Messages>` block at `indent` spaces. Emits nothing when no
    /// messages were registered.
    pub fn write_messages(&self, indent: usize, out: &mut String) {
        if self.messages.is_empty() {
            return;
        }
        let l0 = " ".repeat(indent);
        let l1 = " ".repeat(indent + 2);
        let _ = writeln!(out, "{l0}<Messages>");
        for m in &self.messages {
            let _ = writeln!(
                out,
                r#"{l1}<Message Id="{prefix}_M-{suffix}" Name="{name}" Text="{text}" />"#,
                prefix = self.app_prefix,
                suffix = m.suffix,
                name = escape_attr(&m.name),
                text = escape_attr(&m.text),
            );
        }
        let _ = writeln!(out, "{l0}</Messages>");
    }
}

#[cfg(test)]
mod tests {
    use super::super::AppProgram;
    use super::*;

    #[test]
    fn catalog_and_hardware_match_ets_bytes() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        app.add_catalog_section(
            CatalogSection::new("SnapDog", "SnapDog", "SnapDog", "de-DE").with_item(
                CatalogItem::new("SnapDog", "1", "0xFF01", 1, "0xFF01", "de-DE"),
            ),
        );
        app.add_hardware(
            Hardware::new("0xFF01", 1, "SnapDog")
                .with_product(Product::new("0xFF01", "SnapDog", "de-DE"))
                .with_program(Hardware2Program::new("MT-0", "0001/1")),
        );
        let mut out = String::new();
        app.write_catalog(6, &mut out);
        app.write_hardware(6, &mut out);
        let expected = concat!(
            "      <Catalog>\n",
            "        <CatalogSection Id=\"M-00FA_CS-SnapDog\" Name=\"SnapDog\" Number=\"SnapDog\" DefaultLanguage=\"de-DE\">\n",
            "          <CatalogItem Id=\"M-00FA_H-0xFF01-1_HP-FF01-01-0000_CI-0xFF01-1\" Name=\"SnapDog\" Number=\"1\" ProductRefId=\"M-00FA_H-0xFF01-1_P-0xFF01\" Hardware2ProgramRefId=\"M-00FA_H-0xFF01-1_HP-FF01-01-0000\" DefaultLanguage=\"de-DE\" />\n",
            "        </CatalogSection>\n",
            "      </Catalog>\n",
            "      <Hardware>\n",
            "        <Hardware Id=\"M-00FA_H-0xFF01-1\" Name=\"SnapDog\" SerialNumber=\"0xFF01\" VersionNumber=\"1\" BusCurrent=\"0\" HasIndividualAddress=\"true\" HasApplicationProgram=\"true\">\n",
            "          <Products>\n",
            "            <Product Id=\"M-00FA_H-0xFF01-1_P-0xFF01\" Text=\"SnapDog\" OrderNumber=\"0xFF01\" IsRailMounted=\"false\" DefaultLanguage=\"de-DE\">\n",
            "              <RegistrationInfo RegistrationStatus=\"Registered\" />\n",
            "            </Product>\n",
            "          </Products>\n",
            "          <Hardware2Programs>\n",
            "            <Hardware2Program Id=\"M-00FA_H-0xFF01-1_HP-FF01-01-0000\" MediumTypes=\"MT-0\">\n",
            "              <ApplicationProgramRef RefId=\"M-00FA_A-FF01-01-0000\" />\n",
            "              <RegistrationInfo RegistrationStatus=\"Registered\" RegistrationNumber=\"0001/1\" />\n",
            "            </Hardware2Program>\n",
            "          </Hardware2Programs>\n",
            "        </Hardware>\n",
            "      </Hardware>\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn messages_options_tables_match_ets_bytes() {
        let mut app = AppProgram::new("M-00FA_A-FF01-01-0000");
        app.add_message(Message::new(
            "1",
            "VersionMismatch",
            "Application and firmware version mismatch.",
        ));
        let mut out = String::new();
        app.write_messages(12, &mut out);
        Options::new("iso-8859-15", true, true).write(12, &mut out);
        write_address_table(12, 2047, &mut out);
        write_association_table(12, 2047, &mut out);
        let expected = concat!(
            "            <Messages>\n",
            "              <Message Id=\"M-00FA_A-FF01-01-0000_M-1\" Name=\"VersionMismatch\" Text=\"Application and firmware version mismatch.\" />\n",
            "            </Messages>\n",
            "            <Options TextParameterEncoding=\"iso-8859-15\" SupportsExtendedMemoryServices=\"true\" SupportsExtendedPropertyServices=\"true\" />\n",
            "            <AddressTable MaxEntries=\"2047\" />\n",
            "            <AssociationTable MaxEntries=\"2047\" />\n",
        );
        assert_eq!(out, expected);
    }
}
