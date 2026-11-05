//! `build.rs` helper: gives a launcher's Windows executable (or a hook DLL)
//! version information, an icon and a manifest.
//!
//! An executable with no name, version, publisher or icon is one of the
//! signals antivirus heuristics weigh; every rea launcher shipped that way
//! until v0.4.3, and a player's antivirus flagged CMRLauncher (Quiver
//! feedback, 2026-11-04). With these resources Windows' Properties dialog
//! and Task Manager also show what the program is.
//!
//! ```no_run
//! // build.rs, in `fn main()`:
//! rea_launcher_build::Resources::new("CMR Launcher", "Colin McRae Rally launcher")
//!     .company("AkitaOnRails")
//!     .file_name("CMRLauncher.exe")
//!     .icon("../../packaging/appimage/cmr-launcher.png")
//!     .embed()
//!     .unwrap();
//! ```
//!
//! `embed` does nothing unless the target is Windows. It compiles the
//! resources with MinGW's `windres` (`$WINDRES`, else
//! `<arch>-w64-mingw32-windres`, else `windres`) and links them into the
//! crate's binaries (or its cdylib with [`Resources::dll`]). The version is
//! the crate's own (`CARGO_PKG_VERSION`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What goes into the executable's resources.
#[derive(Debug, Clone)]
pub struct Resources {
    product: String,
    description: String,
    company: String,
    copyright: String,
    icon: Option<PathBuf>,
    file_name: Option<String>,
    dll: bool,
}

impl Resources {
    /// `product` (Windows' "Product name") and `description` ("File
    /// description", what Task Manager shows).
    pub fn new(product: &str, description: &str) -> Resources {
        Resources {
            product: product.into(),
            description: description.into(),
            company: String::new(),
            copyright: String::new(),
            icon: None,
            file_name: None,
            dll: false,
        }
    }

    /// The publisher ("Company").
    pub fn company(mut self, company: &str) -> Resources {
        self.company = company.into();
        if self.copyright.is_empty() {
            self.copyright = format!("Copyright (c) {company}. MIT OR Apache-2.0.");
        }
        self
    }

    /// "Legal copyright"; defaults to the company's under MIT OR Apache-2.0.
    pub fn copyright(mut self, copyright: &str) -> Resources {
        self.copyright = copyright.into();
        self
    }

    /// The icon, a PNG (the AppImage's), relative to the crate's folder.
    pub fn icon(mut self, png: impl Into<PathBuf>) -> Resources {
        self.icon = Some(png.into());
        self
    }

    /// The file's name as shipped (`CMRLauncher.exe`): Windows' "Original
    /// filename". Defaults to the package name with `.exe` or `.dll`.
    pub fn file_name(mut self, name: &str) -> Resources {
        self.file_name = Some(name.into());
        self
    }

    /// For a DLL (a hook library): link into the cdylib, no manifest.
    pub fn dll(mut self) -> Resources {
        self.dll = true;
        self
    }

    /// Compiles and links the resources when building for Windows.
    pub fn embed(&self) -> Result<(), String> {
        println!("cargo:rerun-if-env-changed=WINDRES");
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            return Ok(());
        }
        let out = PathBuf::from(env("OUT_DIR")?);
        let manifest_dir = PathBuf::from(env("CARGO_MANIFEST_DIR")?);
        let version = env("CARGO_PKG_VERSION")?;
        let file_name = match &self.file_name {
            Some(name) => name.clone(),
            None => format!(
                "{}.{}",
                env("CARGO_PKG_NAME")?,
                if self.dll { "dll" } else { "exe" }
            ),
        };
        let icon = match &self.icon {
            Some(png) => {
                let png = manifest_dir.join(png);
                println!("cargo:rerun-if-changed={}", png.display());
                let bytes = std::fs::read(&png)
                    .map_err(|e| format!("cannot read the icon {}: {e}", png.display()))?;
                let ico = out.join("rea-app.ico");
                std::fs::write(&ico, ico_from_png(&bytes)?)
                    .map_err(|e| format!("cannot write {}: {e}", ico.display()))?;
                Some(ico)
            }
            None => None,
        };
        let manifest = if self.dll {
            None
        } else {
            let path = out.join("rea-app.manifest");
            std::fs::write(&path, manifest_xml())
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            Some(path)
        };
        let rc = out.join("rea-app.rc");
        let text = self.rc_text(&version, &file_name, icon.as_deref(), manifest.as_deref());
        std::fs::write(&rc, text).map_err(|e| format!("cannot write {}: {e}", rc.display()))?;
        let object = out.join("rea-app-res.o");
        let windres = windres();
        let status = Command::new(&windres)
            .args(["-O", "coff", "-i"])
            .arg(&rc)
            .arg("-o")
            .arg(&object)
            .status()
            .map_err(|e| format!("cannot run {windres} (MinGW's resource compiler): {e}"))?;
        if !status.success() {
            return Err(format!("{windres} failed on {}", rc.display()));
        }
        let kind = if self.dll { "cdylib" } else { "bins" };
        println!("cargo:rustc-link-arg-{kind}={}", object.display());
        Ok(())
    }

    /// The `.rc` source.
    pub fn rc_text(
        &self,
        version: &str,
        file_name: &str,
        icon: Option<&Path>,
        manifest: Option<&Path>,
    ) -> String {
        let numbers = version_numbers(version);
        let comma = numbers.map(|n| n.to_string()).join(",");
        let mut rc = String::new();
        let _ = writeln!(rc, "1 VERSIONINFO");
        let _ = writeln!(rc, "FILEVERSION {comma}");
        let _ = writeln!(rc, "PRODUCTVERSION {comma}");
        let _ = writeln!(rc, "FILEFLAGSMASK 0x3f");
        let _ = writeln!(rc, "FILEFLAGS 0x0");
        let _ = writeln!(rc, "FILEOS 0x40004");
        let _ = writeln!(rc, "FILETYPE {}", if self.dll { "0x2" } else { "0x1" });
        let _ = writeln!(rc, "FILESUBTYPE 0x0");
        let _ = writeln!(rc, "BEGIN");
        let _ = writeln!(rc, "  BLOCK \"StringFileInfo\"");
        let _ = writeln!(rc, "  BEGIN");
        let _ = writeln!(rc, "    BLOCK \"040904b0\"");
        let _ = writeln!(rc, "    BEGIN");
        let internal = file_name
            .rsplit_once('.')
            .map_or(file_name, |(stem, _)| stem);
        for (key, value) in [
            ("CompanyName", self.company.as_str()),
            ("FileDescription", self.description.as_str()),
            ("FileVersion", version),
            ("InternalName", internal),
            ("LegalCopyright", self.copyright.as_str()),
            ("OriginalFilename", file_name),
            ("ProductName", self.product.as_str()),
            ("ProductVersion", version),
        ] {
            let _ = writeln!(rc, "      VALUE \"{key}\", \"{}\"", rc_string(value));
        }
        let _ = writeln!(rc, "    END");
        let _ = writeln!(rc, "  END");
        let _ = writeln!(rc, "  BLOCK \"VarFileInfo\"");
        let _ = writeln!(rc, "  BEGIN");
        let _ = writeln!(rc, "    VALUE \"Translation\", 0x409, 1200");
        let _ = writeln!(rc, "  END");
        let _ = writeln!(rc, "END");
        if let Some(icon) = icon {
            let _ = writeln!(rc, "1 ICON \"{}\"", rc_path(icon));
        }
        if let Some(manifest) = manifest {
            // RT_MANIFEST (24), id 1: the executable's own manifest.
            let _ = writeln!(rc, "1 24 \"{}\"", rc_path(manifest));
        }
        rc
    }
}

fn env(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is not set (run from build.rs)"))
}

/// The resource compiler for the target.
fn windres() -> String {
    if let Ok(path) = std::env::var("WINDRES") {
        return path;
    }
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86") => "i686",
        Ok("x86_64") => "x86_64",
        Ok("aarch64") => "aarch64",
        _ => return "windres".into(),
    };
    let prefixed = format!("{arch}-w64-mingw32-windres");
    let found = std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join(&prefixed).is_file())
    });
    if found { prefixed } else { "windres".into() }
}

/// `X.Y.Z` (pre-release and build parts dropped) as four numbers.
pub fn version_numbers(version: &str) -> [u16; 4] {
    let core = version.split(['-', '+']).next().unwrap_or("");
    let mut numbers = [0u16; 4];
    for (slot, part) in numbers.iter_mut().zip(core.split('.')) {
        *slot = part.parse().unwrap_or(0);
    }
    numbers
}

/// A string literal's body for `.rc`: quotes doubled, no newlines.
fn rc_string(text: &str) -> String {
    text.replace('"', "\"\"").replace(['\r', '\n'], " ")
}

/// A path in `.rc`: forward slashes (windres reads them on every host).
fn rc_path(path: &Path) -> String {
    rc_string(&path.display().to_string().replace('\\', "/"))
}

/// The executable's manifest: runs as the user (no elevation prompt), and
/// the Windows versions it was written for. Nothing that changes how the
/// launcher behaves (DPI and code page stay as the program sets them).
pub fn manifest_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>
"#
}

/// An `.ico` holding `png` as its one image (Windows Vista and later read
/// PNG-compressed icons).
pub fn ico_from_png(png: &[u8]) -> Result<Vec<u8>, String> {
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" || &png[12..16] != b"IHDR" {
        return Err("the icon is not a PNG".into());
    }
    let side = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
    let (width, height) = (side(16), side(20));
    if width == 0 || height == 0 || width > 256 || height > 256 {
        return Err(format!(
            "the icon is {width}x{height}; it must be at most 256x256"
        ));
    }
    // 256 is written as 0 in the directory entry.
    let byte = |n: u32| (n % 256) as u8;
    let mut ico = Vec::with_capacity(22 + png.len());
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    ico.extend_from_slice(&[byte(width), byte(height), 0, 0]);
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&32u16.to_le_bytes());
    ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(png);
    Ok(ico)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    #[test]
    fn the_icon_wraps_the_png() {
        let image = png(256, 256);
        let ico = ico_from_png(&image).unwrap();
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!(&ico[6..8], &[0, 0], "256 is written as 0");
        assert_eq!(
            u32::from_le_bytes(ico[14..18].try_into().unwrap()),
            image.len() as u32
        );
        assert_eq!(u32::from_le_bytes(ico[18..22].try_into().unwrap()), 22);
        assert_eq!(&ico[22..], &image[..]);
        assert_eq!(&ico_from_png(&png(48, 32)).unwrap()[6..8], &[48, 32]);
        assert!(ico_from_png(b"GIF89a").is_err());
        assert!(ico_from_png(&png(512, 512)).is_err());
    }

    #[test]
    fn versions_become_four_numbers() {
        assert_eq!(version_numbers("0.9.1"), [0, 9, 1, 0]);
        assert_eq!(version_numbers("1.2.3-rc.1+build"), [1, 2, 3, 0]);
        assert_eq!(version_numbers("junk"), [0, 0, 0, 0]);
    }

    #[test]
    fn the_rc_names_the_program_and_its_files() {
        let rc = Resources::new("CMR \"Launcher\"", "Colin McRae Rally launcher")
            .company("AkitaOnRails")
            .rc_text(
                "0.9.1",
                "CMRLauncher.exe",
                Some(Path::new(r"C:\out\rea-app.ico")),
                Some(Path::new("/out/rea-app.manifest")),
            );
        assert!(rc.contains("FILEVERSION 0,9,1,0"));
        assert!(rc.contains("FILETYPE 0x1"));
        assert!(rc.contains("VALUE \"ProductName\", \"CMR \"\"Launcher\"\"\""));
        assert!(rc.contains("VALUE \"FileDescription\", \"Colin McRae Rally launcher\""));
        assert!(rc.contains("VALUE \"CompanyName\", \"AkitaOnRails\""));
        assert!(rc.contains(
            "VALUE \"LegalCopyright\", \"Copyright (c) AkitaOnRails. MIT OR Apache-2.0.\""
        ));
        assert!(rc.contains("VALUE \"OriginalFilename\", \"CMRLauncher.exe\""));
        assert!(rc.contains("VALUE \"InternalName\", \"CMRLauncher\""));
        assert!(rc.contains("1 ICON \"C:/out/rea-app.ico\""));
        assert!(rc.contains("1 24 \"/out/rea-app.manifest\""));
        let dll = Resources::new("CMR hooks", "Colin McRae Rally fixes")
            .dll()
            .rc_text("0.9.1", "cmrhooks.dll", None, None);
        assert!(dll.contains("FILETYPE 0x2"));
        assert!(!dll.contains("ICON") && !dll.contains("1 24"));
    }

    #[test]
    fn the_manifest_asks_for_no_elevation() {
        let xml = manifest_xml();
        assert!(xml.contains("level=\"asInvoker\""));
        assert!(
            !xml.contains("dpiAware"),
            "DPI handling stays the program's"
        );
    }

    #[test]
    fn nothing_happens_for_other_targets() {
        // build.rs on Linux: no windres, no link arguments, no error.
        if std::env::var("CARGO_CFG_TARGET_OS").is_err() {
            assert!(Resources::new("x", "y").embed().is_ok());
        }
    }
}
