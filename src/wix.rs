// Copyright (C) 2017 Christopher R. Field.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use mustache::{self, MapBuilder};
use regex::Regex;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use super::{Error, Platform, Result, TimestampServer};
use toml::Value;

pub const CARGO_MANIFEST_FILE: &str = "Cargo.toml";
pub const CARGO: &str = "cargo";
pub const DEFAULT_LICENSE_FILE_NAME: &str = "LICENSE";
pub const SIGNTOOL: &str = "signtool";
pub const WIX: &str = "wix";
pub const WIX_COMPILER: &str = "candle";
pub const WIX_LINKER: &str = "light";
pub const WIX_PATH_KEY: &str = "WIX_PATH";
pub const WIX_SOURCE_FILE_EXTENSION: &str = "wxs";
pub const WIX_SOURCE_FILE_NAME: &str = "main";

/// The MIT Rich Text Format (RTF) license template.
static MIT_LICENSE_TEMPLATE: &str = include_str!("MIT.rtf");

/// The Apache-2.0 Rich Text Format (RTF) license template.
static APACHE2_LICENSE_TEMPLATE: &str = include_str!("Apache-2.0.rtf");

/// The GPL-3.0 Rich Text Format (RTF) license template.
static GPL3_LICENSE_TEMPLATE: &str = include_str!("GPL-3.0.rtf");

/// A builder for running the subcommand.
#[derive(Debug, Clone)]
pub struct Wix<'a> {
    bin_path: Option<&'a str>,
    binary_name: Option<&'a str>,
    capture_output: bool,
    copyright_year: Option<&'a str>,
    copyright_holder: Option<&'a str>,
    description: Option<&'a str>,
    input: Option<&'a str>,
    license_path: Option<&'a str>,
    manufacturer: Option<&'a str>,
    product_name: Option<&'a str>,
    sign: bool,
    sign_path: Option<&'a str>,
    timestamp: Option<&'a str>,
}

impl<'a> Wix<'a> {
    /// Creates a new `Wix` instance.
    pub fn new() -> Self {
        Wix {
            bin_path: None,
            binary_name: None,
            capture_output: true,
            copyright_year: None,
            copyright_holder: None,
            description: None,
            input: None,
            license_path: None,
            manufacturer: None,
            product_name: None,
            sign: false,
            sign_path: None,
            timestamp: None,
        }
    }

    /// Sets the path to the WiX Toolset's `bin` folder.
    ///
    /// The WiX Toolset's `bin` folder should contain the needed `candle.exe` and `light.exe`
    /// applications. The default is to use the PATH system environment variable. This will
    /// override any value obtained from the environment.
    pub fn bin_path(mut self, b: Option<&'a str>) -> Self {
        self.bin_path = b;
        self
    }

    /// Sets the binary name.
    ///
    /// This overrides the binary name determined from the package's manifest (Cargo.toml).
    pub fn binary_name(mut self, b: Option<&'a str>) -> Self {
        self.binary_name = b;
        self
    }

    /// Enables or disables capturing of the output from the builder (`cargo`), compiler
    /// (`candle`), linker (`light`), and signer (`signtool`).
    ///
    /// The default is to capture all output, i.e. display nothing in the console but the log
    /// statements.
    pub fn capture_output(mut self, c: bool) -> Self {
        self.capture_output = c;
        self
    }

    /// Sets the copyright holder in the license dialog of the Windows installer (msi).
    pub fn copyright_holder(mut self, h: Option<&'a str>) -> Self {
        self.copyright_holder = h;
        self
    }

    /// Sets the copyright year in the license dialog of the Windows installer (msi).
    pub fn copyright_year(mut self, y: Option<&'a str>) -> Self {
        self.copyright_year = y;
        self
    }

    /// Sets the description.
    ///
    /// This override the description determined from the `description` field in the package's
    /// manifest (Cargo.toml).
    pub fn description(mut self, d: Option<&'a str>) -> Self {
        self.description = d;
        self
    }

    /// Sets the path to a file to be used as the WiX Source (wxs) file instead of `wix\main.rs`.
    pub fn input(mut self, i: Option<&'a str>) -> Self {
        self.input = i;
        self
    }
    
    /// Sets the path to a file to used as the `License.txt` file within the installer.
    ///
    /// The `License.txt` file is installed into the installation location along side the `bin`
    /// folder. Note, the file can be in any format with any name, but it is automatically renamed
    /// to `License.txt` during creation of the installer.
    pub fn license_file(mut self, l: Option<&'a str>) -> Self {
        self.license_path = l;
        self
    }

    /// Overrides the first author in the `authors` field of the package's manifest (Cargo.toml) as
    /// the manufacturer within the installer.
    pub fn manufacturer(mut self, m: Option<&'a str>) -> Self {
        self.manufacturer = m;
        self
    }

    /// Sets the product name.
    ///
    /// This override the product name determined from the `name` field in the package's
    /// manifest (Cargo.toml).
    pub fn product_name(mut self, p: Option<&'a str>) -> Self {
        self.product_name = p;
        self
    }

    /// Enables or disables signing of the installer after creation with the `signtool`
    /// application.
    pub fn sign(mut self, s: bool) -> Self {
        self.sign = s;
        self
    }

    /// Sets the path to the folder containing the `signtool.exe` file.
    ///
    /// Normally the `signtool.exe` is installed in the `bin` folder of the Windows SDK
    /// installation. THe default is to use the PATH system environment variable. This will
    /// override any value obtained from the environment.
    pub fn sign_path(mut self, s: Option<&'a str>) -> Self {
        self.sign_path = s;
        self
    }

    /// Sets the URL for the timestamp server used when signing an installer.
    ///
    /// The default is to _not_ use a timestamp server, even though it is highly recommended. Use
    /// this method to enable signing with the timestamp.
    pub fn timestamp(mut self, t: Option<&'a str>) -> Self {
        self.timestamp = t;
        self
    }

    /// Creates the necessary sub-folders and files to immediately use the `cargo wix` subcommand to
    /// create an installer for the package.
    pub fn init(self, force: bool) -> Result<()> {
        let mut main_wxs_path = PathBuf::from(WIX);
        if !main_wxs_path.exists() {
            fs::create_dir(&main_wxs_path)?;
        }
        main_wxs_path.push(WIX_SOURCE_FILE_NAME);
        main_wxs_path.set_extension(WIX_SOURCE_FILE_EXTENSION);
        if main_wxs_path.exists() && !force {
            Err(Error::Generic(
                format!("The '{}' file already exists. Use the '--force' flag to overwrite the contents.", 
                    main_wxs_path.display())
            ))
        } else {
            let mut main_wxs = File::create(main_wxs_path)?;
            super::write_wix_source(&mut main_wxs)?;
            Ok(())
        }
    }
   
    /// Runs the subcommand to build the release binary, compile, link, and possibly sign the installer
    /// (msi).
    pub fn run(self) -> Result<()> {
        debug!("binary_name = {:?}", self.binary_name);
        debug!("capture_output = {:?}", self.capture_output);
        debug!("description = {:?}", self.description);
        debug!("input = {:?}", self.input);
        debug!("manufacturer = {:?}", self.manufacturer);
        debug!("product_name = {:?}", self.product_name);
        debug!("sign = {:?}", self.sign);
        debug!("timestamp = {:?}", self.timestamp);
        let cargo_file_path = Path::new(CARGO_MANIFEST_FILE);
        debug!("cargo_file_path = {:?}", cargo_file_path);
        let mut cargo_file = File::open(cargo_file_path)?;
        let mut cargo_file_content = String::new();
        cargo_file.read_to_string(&mut cargo_file_content)?;
        let manifest = cargo_file_content.parse::<Value>()?;
        let pkg_version = self.get_version(&manifest)?;
        debug!("pkg_version = {:?}", pkg_version);
        let product_name = self.get_product_name(&manifest)?;
        debug!("product_name = {:?}", product_name);
        let description = self.get_description(&manifest)?;
        debug!("description = {:?}", description);
        let homepage = manifest.get("package")
            .and_then(|p| p.as_table())
            .and_then(|t| t.get("homepage"))
            .and_then(|d| d.as_str());
        debug!("homepage = {:?}", homepage);
        let license_name = self.get_license_name(&manifest)?;
        debug!("license_name = {:?}", license_name);
        let license_source = self.get_license_source(&manifest);
        debug!("license_source = {:?}", license_source);
        let manufacturer = self.get_manufacturer(&manifest)?;
        debug!("manufacturer = {}", manufacturer);
        let help_url = manifest
            .get("package")
            .and_then(|p| p.as_table())
            .and_then(|t| t.get("documentation").or(t.get("homepage")).or(t.get("repository")))
            .and_then(|h| h.as_str())
            .ok_or(Error::Manifest(String::from("documentation")))?;
        debug!("help_url = {:?}", help_url);
        let binary_name = self.get_binary_name(&manifest)?;
        debug!("binary_name = {:?}", binary_name);
        let platform = if cfg!(target_arch = "x86_64") {
            Platform::X64
        } else {
            Platform::X86
        };
        debug!("platform = {:?}", platform);
        let source_wxs = self.get_wxs_source()?;
        debug!("source_wxs = {:?}", source_wxs);
        let mut source_wixobj = PathBuf::from("target");
        source_wixobj.push(WIX);
        source_wixobj.push("build");
        source_wixobj.push(WIX_SOURCE_FILE_NAME);
        source_wixobj.set_extension("wixobj");
        debug!("source_wixobj = {:?}", source_wixobj);
        let mut destination_msi = PathBuf::from("target");
        destination_msi.push(WIX);
        // Do NOT use the `set_extension` method for the MSI path. Since the pkg_version is in X.X.X
        // format, the `set_extension` method will replace the Patch version number and
        // architecture/platform with `msi`.  Instead, just include the extension in the formatted
        // name.
        destination_msi.push(&format!("{}-{}-{}.msi", product_name, pkg_version, platform.arch()));
        debug!("destination_msi = {:?}", destination_msi);
        // Build the binary with the release profile. If a release binary has already been built, then
        // this will essentially do nothing.
        info!("Building release binary");
        let mut builder = Command::new(CARGO);
        if self.capture_output {
            trace!("Capturing the '{}' output", CARGO);
            builder.stdout(Stdio::null());
            builder.stderr(Stdio::null());
        }
        let status = builder.arg("build").arg("--release").status()?;
        if !status.success() {
            return Err(Error::Command(CARGO, status.code().unwrap_or(0)));
        }
        // Compile the installer
        info!("Compiling installer");
        let mut compiler = self.get_compiler();
        debug!("compiler = {:?}", compiler);
        if self.capture_output {
            trace!("Capturing the '{}' output", WIX_COMPILER);
            compiler.stdout(Stdio::null());
            compiler.stderr(Stdio::null());
        } 
        let status = compiler
            .arg(format!("-dVersion={}", pkg_version))
            .arg(format!("-dPlatform={}", platform))
            .arg(format!("-dProductName={}", product_name))
            .arg(format!("-dBinaryName={}", binary_name))
            .arg(format!("-dDescription={}", description))
            .arg(format!("-dManufacturer={}", manufacturer))
            .arg(format!("-dLicenseName={}", license_name))
            .arg(format!("-dLicenseSource={}", license_source.display()))
            .arg(format!("-dHelp={}", help_url))
            .arg("-o")
            .arg(&source_wixobj)
            .arg(&source_wxs)
            .status()?;
        if !status.success() {
            return Err(Error::Command(WIX_COMPILER, status.code().unwrap_or(0)));
        }
        // Link the installer
        info!("Linking the installer");
        let mut linker = self.get_linker(); 
        debug!("linker = {:?}", linker);
        if self.capture_output {
            trace!("Capturing the '{}' output", WIX_LINKER);
            linker.stdout(Stdio::null());
            linker.stderr(Stdio::null());
        }
        let status = linker
            .arg("-ext")
            .arg("WixUIExtension")
            .arg("-cultures:en-us")
            .arg(&source_wixobj)
            .arg("-out")
            .arg(&destination_msi)
            .status()?;
        if !status.success() {
            return Err(Error::Command(WIX_LINKER, status.code().unwrap_or(0)));
        }
        // Sign the installer
        if self.sign {
            info!("Signing the installer");
            let mut signer = self.get_signer();
            debug!("signer = {:?}", signer);
            if self.capture_output {
                trace!("Capturing the {} output", SIGNTOOL);
                signer.stdout(Stdio::null());
                signer.stderr(Stdio::null());
            }
            signer.arg("sign")
                .arg("/a")
                .arg("/d")
                .arg(format!("{} - {}", product_name, description));
            if let Some(h) = homepage {
                trace!("Using the '{}' URL for the expanded description", h);
                signer.arg("/du").arg(h);
            }
            if let Some(t) = self.timestamp {
                let server = TimestampServer::from_str(&t)?;
                trace!("Using the '{}' timestamp server to sign the installer", server); 
                signer.arg("/t");
                signer.arg(server.url());
            }
            let status = signer.arg(&destination_msi).status()?;
            if !status.success() {
                return Err(Error::Command(SIGNTOOL, status.code().unwrap_or(0)));
            }
        }
        Ok(())
    }

    /// Gets the binary name.
    ///
    /// This is the name of the executable (exe) file.
    fn get_binary_name(&self, manifest: &Value) -> Result<String> {
        if let Some(b) = self.binary_name {
            Ok(b.to_owned())
        } else {
            manifest.get("bin")
                .and_then(|b| b.as_table())
                .and_then(|t| t.get("name")) 
                .and_then(|n| n.as_str())
                .map_or(self.get_product_name(manifest), |s| Ok(String::from(s)))
        }
    }

    /// Gets the command for the compiler application (`candle.exe`).
    fn get_compiler(&self) -> Command {
        if let Some(b) = self.bin_path {
            trace!("Using the '{}' path to the WiX Toolset compiler", b);
            Command::new(PathBuf::from(b).join(WIX_COMPILER))
        } else {
            env::var(WIX_PATH_KEY).map(|s| {
                trace!("Using the '{}' path to the WiX Toolset compiler", s);
                Command::new(PathBuf::from(s).join(WIX_COMPILER))
            }).unwrap_or(Command::new(WIX_COMPILER))
        }
    }

    /// Gets the copyright holder.
    ///
    /// If no copyright holder was explicitly specified, then the manufacturer is used.
    fn get_copyright_holder(&self, manifest: &Value) -> Result<String> {
        if let Some(y) = self.copyright_year {
            Ok(y.to_owned())
        } else {
            self.get_manufacturer(manifest)
        }
    }

    /// Gets the copyright year.
    fn get_copyright_year(&self) -> String {
        if let Some(c) = self.copyright_year {
            c.to_owned()
        } else {
            String::from("YYYY")
        }
    }

    /// Gets the description.
    ///
    /// If no description is explicitly set using the builder pattern, then the description from
    /// the package's manifest (Cargo.toml) is used.
    fn get_description(&self, manifest: &Value) -> Result<String> {
        if let Some(d) = self.description {
            Ok(d.to_owned())
        } else {
            manifest.get("package")
                .and_then(|p| p.as_table())
                .and_then(|t| t.get("description"))
                .and_then(|d| d.as_str())
                .map(|s| String::from(s))
                .ok_or(Error::Manifest(String::from("description")))
        }
    }

    /// Gets the license name.
    ///
    /// If a path to a license file is specified, then the file name will be used. If no license
    /// path is specified, then the file name from the `license-file` field in the package's
    /// manifest is used.
    fn get_license_name(&self, manifest: &Value) -> Result<String> {
        if let Some(l) = self.license_path.map(|s| PathBuf::from(s)) {
            l.file_name()
                .and_then(|f| f.to_str())
                .map(|s| String::from(s))
                .ok_or(Error::Generic(
                    format!("The '{}' license path does not contain a file name.", l.display())
                ))
        } else {
            manifest.get("package")
                .and_then(|p| p.as_table())
                .and_then(|t| t.get("license-file"))
                .and_then(|l| l.as_str())
                .and_then(|s| Path::new(s).file_name().and_then(|f| f.to_str()))
                .or(Some("License.txt"))
                .map(|s| String::from(s))
                .ok_or(Error::Generic(
                    format!("The 'license-file' field value does not contain a file name.")
                )) 
        }
    }

    /// Gets the license source file.
    ///
    /// This is the license file that is placed in the installation location alongside the `bin`
    /// folder for the executable (exe) file.
    fn get_license_source(&self, manifest: &Value) -> PathBuf {
        self.license_path.map(|l| PathBuf::from(l)).unwrap_or(
            // TODO: Add generation of license file from `license` field
            manifest.get("package")
                .and_then(|p| p.as_table())
                .and_then(|t| t.get("license-file"))
                .and_then(|l| l.as_str())
                .map(|s| PathBuf::from(s))
                .unwrap_or(PathBuf::from(DEFAULT_LICENSE_FILE_NAME))
        )
    }

    /// Gets the command for the linker application (`light.exe`).
    fn get_linker(&self) -> Command {
        if let Some(b) = self.bin_path {
            trace!("Using the '{}' path to the WiX Toolset linker", b);
            Command::new(PathBuf::from(b).join(WIX_LINKER))
        } else {
            env::var(WIX_PATH_KEY).map(|s| {
                trace!("Using the '{}' path to the WiX Toolset linker", s);
                Command::new(PathBuf::from(s).join(WIX_LINKER))
            }).unwrap_or(Command::new(WIX_LINKER))
        }
    }

    /// Gets the manufacturer.
    ///
    /// The manufacturer is displayed in the Add/Remove Programs (ARP) control panel under the
    /// "Publisher" column. If no manufacturer is explicitly set using the builder pattern, then
    /// the first author from the `authors` field in the package's manifest (Cargo.toml) is used.
    fn get_manufacturer(&self, manifest: &Value) -> Result<String> {
        if let Some(m) = self.manufacturer {
            Ok(m.to_owned())
        } else {
            manifest.get("package")
                .and_then(|p| p.as_table())
                .and_then(|t| t.get("authors"))
                .and_then(|a| a.as_array())
                .and_then(|a| a.get(0)) 
                .and_then(|f| f.as_str())
                .and_then(|s| {
                    // Strip email if it exists.
                    let re = Regex::new(r"<(.*?)>").unwrap();
                    Some(re.replace_all(s, ""))
                })
                .map(|s| String::from(s.trim()))
                .ok_or(Error::Manifest(String::from("authors")))
        }
    }

    /// Gets the product name.
    fn get_product_name(&self, manifest: &Value) -> Result<String> {
        if let Some(p) = self.product_name {
            Ok(p.to_owned())
        } else {
            manifest.get("package")
                .and_then(|p| p.as_table())
                .and_then(|t| t.get("name"))
                .and_then(|n| n.as_str())
                .map(|s| String::from(s))
                .ok_or(Error::Manifest(String::from("name")))
        }
    }

    /// Gets the command for the `signtool` application.
    fn get_signer(&self) -> Command {
        if let Some(s) = self.sign_path {
            trace!("Using the '{}' path to the Windows SDK signtool", s);
            Command::new(PathBuf::from(s).join(SIGNTOOL))
        } else {
            Command::new(SIGNTOOL)
        }
    }

    /// Gets the WiX Source (wxs) file.
    fn get_wxs_source(&self) -> Result<PathBuf> {
        if let Some(p) = self.input.map(|s| PathBuf::from(s)) {
            if p.exists() {
                if p.is_dir() {
                    Err(Error::Generic(format!("The '{}' path is not a file. Please check the path and ensure it is to a WiX Source (wxs) file.", p.display())))
                } else {
                    trace!("Using the '{}' WiX source file", p.display());
                    Ok(p)
                }
            } else {
                Err(Error::Generic(format!("The '{0}' file does not exist. Consider using the 'cargo wix --print-template > {0}' command to create it.", p.display())))
            }
        } else {
            trace!("Using the default WiX source file");
            let mut main_wxs = PathBuf::from(WIX);
            main_wxs.push(WIX_SOURCE_FILE_NAME);
            main_wxs.set_extension(WIX_SOURCE_FILE_EXTENSION);
            if main_wxs.exists() {
                Ok(main_wxs)
            } else {
               Err(Error::Generic(format!("The '{0}' file does not exist. Consider using the 'cargo wix --init' command to create it.", main_wxs.display())))
            }
        }
    }

    /// Gets the package version.
    fn get_version(&self, cargo_toml: &Value) -> Result<String> {
        cargo_toml.get("package")
            .and_then(|p| p.as_table())
            .and_then(|t| t.get("version"))
            .and_then(|v| v.as_str())
            .map(|s| String::from(s))
            .ok_or(Error::Manifest(String::from("version")))
    }

    /// Renders the MIT license in the Rich Text Format (RTF).
    ///
    /// This will be automatically included in the Windows installer (msi) and displayed in the license
    /// dialog.
    fn write_mit_license<W: Write>(&self, writer: &mut W, manifest: &Value) -> Result<()> {
        trace!("Writing MIT license");
        let template = mustache::compile_str(MIT_LICENSE_TEMPLATE)?;
        let data = MapBuilder::new()
            .insert_str("copyright-holder", self.get_copyright_holder(manifest)?)
            .insert_str("copyright-year", self.get_copyright_year())
            .insert_str("product-name", self.get_product_name(manifest)?)
            .build();
        template.render_data(writer, &data)?;
        Ok(())
    }
}

impl<'a> Default for Wix<'a> {
    fn default() -> Self {
        Wix::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_correct() {
        let wix = Wix::new();
        assert_eq!(wix.bin_path, None);
        assert_eq!(wix.binary_name, None);
        assert!(wix.capture_output);
        assert_eq!(wix.description, None);
        assert_eq!(wix.input, None);
        assert_eq!(wix.license_path, None);
        assert_eq!(wix.manufacturer, None);
        assert_eq!(wix.product_name, None);
        assert!(!wix.sign);
        assert_eq!(wix.timestamp, None);
    }

    #[test]
    fn bin_path_works() {
        const EXPECTED: &str = "C:\\WiX Toolset\\bin";
        let wix = Wix::new().bin_path(Some(EXPECTED));
        assert_eq!(wix.bin_path, Some(PathBuf::from(EXPECTED)));
    }

    #[test]
    fn binary_name_works() {
        const EXPECTED: &str = "test";
        let wix = Wix::new().binary_name(Some(EXPECTED));
        assert_eq!(wix.binary_name, Some(String::from(EXPECTED)));
    }

    #[test]
    fn capture_output_works() {
        let wix = Wix::new().capture_output(false);
        assert!(!wix.capture_output);
    }

    #[test]
    fn description_works() {
        const EXPECTED: &str = "test description";
        let wix = Wix::new().description(Some(EXPECTED));
        assert_eq!(wix.description, Some(String::from(EXPECTED)));
    }

    #[test]
    fn input_works() {
        const EXPECTED: &str = "test.wxs";
        let wix = Wix::new().input(Some(EXPECTED));
        assert_eq!(wix.input, Some(PathBuf::from(EXPECTED)));
    }

    #[test]
    fn license_file_works() {
        const EXPECTED: &str = "MIT-LICENSE";
        let wix = Wix::new().license_file(Some(EXPECTED));
        assert_eq!(wix.license_path, Some(PathBuf::from(EXPECTED)));
    }

    #[test]
    fn manufacturer_works() {
        const EXPECTED: &str = "Tester";
        let wix = Wix::new().manufacturer(Some(EXPECTED));
        assert_eq!(wix.manufacturer, Some(String::from(EXPECTED)));
    }

    #[test]
    fn product_name_works() {
        const EXPECTED: &str = "Test Product Name";
        let wix = Wix::new().product_name(Some(EXPECTED));
        assert_eq!(wix.product_name, Some(String::from(EXPECTED)));
    }

    #[test]
    fn sign_works() {
        let wix = Wix::new().sign(true);
        assert!(wix.sign);
    }

    #[test]
    fn sign_path_works() {
        const EXPECTED: &str = "C:\\Program Files\\Windows Kit\\bin";
        let wix = Wix::new().sign_path(Some(EXPECTED));
        assert_eq!(wix.sign_path, Some(PathBuf::from(EXPECTED)));
    }

    #[test]
    fn timestamp_works() {
        const EXPECTED: &str = "http://timestamp.comodoca.com/";
        let wix = Wix::new().timestamp(Some(EXPECTED));
        assert_eq!(wix.timestamp, Some(String::from(EXPECTED)));
    }

    #[test]
    fn strip_email_works() {
        const EXPECTED: &str = "Christopher R. Field";
        let re = Regex::new(r"<(.*?)>").unwrap();
        let actual = re.replace_all("Christopher R. Field <cfield2@gmail.com>", "");
        assert_eq!(actual.trim(), EXPECTED);
    }

    #[test]
    fn strip_email_works_without_email() {
        const EXPECTED: &str = "Christopher R. Field";
        let re = Regex::new(r"<(.*?)>").unwrap();
        let actual = re.replace_all("Christopher R. Field", "");
        assert_eq!(actual.trim(), EXPECTED);
    }

    #[test]
    fn strip_email_works_with_only_email() {
        const EXPECTED: &str = "cfield2@gmail.com";
        let re = Regex::new(r"<(.*?)>").unwrap();
        let actual = re.replace_all("cfield2@gmail.com", "");
        assert_eq!(actual.trim(), EXPECTED);
    }
}
