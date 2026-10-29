//! What an app's bundle holds for Finder's own menu: the native extension,
//! `context-menu-finder`, twice. Once as a Finder Sync extension, which
//! Finder runs in the sandbox, and once as its helper, which runs the items'
//! programs, since the sandbox does not let the extension start them. The
//! two share an app group, whose folder carries the extension's requests to
//! the helper. See `docs/macos.md`.
//!
//! This is plain text, the same on every system, for an app's build.

/// The key in the extension's and the helper's `Info.plist` naming the menu
/// they serve.
pub const MENU_KEY: &str = "ContextMenuRsMenu";

/// The key in the extension's and the helper's `Info.plist` naming the app
/// group they share.
pub const GROUP_KEY: &str = "ContextMenuRsGroup";

/// Where the extension goes in the app's bundle.
pub const EXTENSION_PATH: &str = "Contents/PlugIns/ContextMenu.appex";

/// Where the helper goes in the app's bundle.
pub const HELPER_PATH: &str = "Contents/Helpers/ContextMenuHelper.app";

/// The program's name in both: `Contents/MacOS/context-menu-finder` in each.
pub const EXECUTABLE: &str = "context-menu-finder";

/// The Objective-C class Finder makes of the extension.
pub const PRINCIPAL_CLASS: &str = "ContextMenuRsFinderSync";

/// The native extension in an app's bundle, serving the menu `menu`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinderExtension {
    /// The menu's id, as `Menu::new` has it.
    pub menu: String,
    /// The app's bundle identifier, which the extension's and the helper's
    /// start with: `com.example.photokit`.
    pub app: String,
    /// The id of the team signing the app, ten letters and digits: the app
    /// group starts with it.
    pub team: String,
    /// The extension's name in System Settings, where the user turns it on
    /// and off: the app's name, say.
    pub name: String,
    /// The extension's and the helper's version, the app's
    /// `CFBundleShortVersionString` and `CFBundleVersion`: the system takes
    /// the extension of the highest version it knows, and an app's should
    /// be the app's own. `1` until set.
    pub version: String,
}

impl FinderExtension {
    /// The extension for the menu `menu` of the app `app`, signed by `team`,
    /// named `name` in System Settings.
    pub fn new(
        menu: impl Into<String>,
        app: impl Into<String>,
        team: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        FinderExtension {
            menu: menu.into(),
            app: app.into(),
            team: team.into(),
            name: name.into(),
            version: "1".into(),
        }
    }

    /// The extension's bundle identifier: `<app>.context-menu`.
    pub fn extension_id(&self) -> String {
        format!("{}.context-menu", self.app)
    }

    /// The helper's bundle identifier: `<app>.context-menu-helper`.
    pub fn helper_id(&self) -> String {
        format!("{}.context-menu-helper", self.app)
    }

    /// The app group the two share: `<team>.<app>.context-menu`. A group
    /// that starts with the team's id needs no provisioning profile.
    pub fn group(&self) -> String {
        format!("{}.{}.context-menu", self.team, self.app)
    }

    /// The extension's `Info.plist`, at `EXTENSION_PATH/Contents/Info.plist`.
    pub fn extension_plist(&self) -> String {
        plist(&format!(
            "\t<key>CFBundleDisplayName</key>\n\
             \t<string>{name}</string>\n\
             {common}\
             \t<key>NSExtension</key>\n\
             \t<dict>\n\
             \t\t<key>NSExtensionAttributes</key>\n\
             \t\t<dict/>\n\
             \t\t<key>NSExtensionPointIdentifier</key>\n\
             \t\t<string>com.apple.FinderSync</string>\n\
             \t\t<key>NSExtensionPrincipalClass</key>\n\
             \t\t<string>{PRINCIPAL_CLASS}</string>\n\
             \t</dict>\n",
            name = xml(&self.name),
            common = self.common(&self.extension_id(), "XPC!"),
        ))
    }

    /// The helper's `Info.plist`, at `HELPER_PATH/Contents/Info.plist`: an
    /// app with no window and no Dock icon.
    pub fn helper_plist(&self) -> String {
        plist(&format!(
            "{common}\
             \t<key>LSBackgroundOnly</key>\n\
             \t<true/>\n",
            common = self.common(&self.helper_id(), "APPL"),
        ))
    }

    /// The extension's entitlements, to sign it with: the sandbox, which
    /// Finder requires; the app group; and reading the menus' definitions,
    /// in `~/Library/Application Support/context-menu-rs`.
    pub fn extension_entitlements(&self) -> String {
        plist(&format!(
            "\t<key>com.apple.security.app-sandbox</key>\n\
             \t<true/>\n\
             {group}\
             \t<key>com.apple.security.temporary-exception.files.home-relative-path.read-only</key>\n\
             \t<array>\n\
             \t\t<string>/Library/Application Support/context-menu-rs/</string>\n\
             \t</array>\n",
            group = self.group_entitlement(),
        ))
    }

    /// The helper's entitlements, to sign it with: the app group alone. It
    /// runs outside the sandbox, as the app does.
    pub fn helper_entitlements(&self) -> String {
        plist(&self.group_entitlement())
    }

    /// The keys both `Info.plist`s have.
    fn common(&self, id: &str, package_type: &str) -> String {
        format!(
            "\t<key>CFBundleIdentifier</key>\n\
             \t<string>{id}</string>\n\
             \t<key>CFBundleExecutable</key>\n\
             \t<string>{EXECUTABLE}</string>\n\
             \t<key>CFBundleName</key>\n\
             \t<string>{EXECUTABLE}</string>\n\
             \t<key>CFBundlePackageType</key>\n\
             \t<string>{package_type}</string>\n\
             \t<key>CFBundleInfoDictionaryVersion</key>\n\
             \t<string>6.0</string>\n\
             \t<key>CFBundleShortVersionString</key>\n\
             \t<string>{version}</string>\n\
             \t<key>CFBundleVersion</key>\n\
             \t<string>{version}</string>\n\
             \t<key>LSMinimumSystemVersion</key>\n\
             \t<string>11.0</string>\n\
             \t<key>{MENU_KEY}</key>\n\
             \t<string>{menu}</string>\n\
             \t<key>{GROUP_KEY}</key>\n\
             \t<string>{group}</string>\n",
            id = xml(id),
            version = xml(&self.version),
            menu = xml(&self.menu),
            group = xml(&self.group()),
        )
    }

    fn group_entitlement(&self) -> String {
        format!(
            "\t<key>com.apple.security.application-groups</key>\n\
             \t<array>\n\
             \t\t<string>{}</string>\n\
             \t</array>\n",
            xml(&self.group())
        )
    }
}

/// A property list of the dictionary's keys and values `body`.
fn plist(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         {body}\
         </dict>\n\
         </plist>\n"
    )
}

fn xml(s: &str) -> String {
    super::xml(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photokit() -> FinderExtension {
        FinderExtension::new(
            "photo-kit",
            "com.example.photokit",
            "AB12CD34EF",
            "Photo & Kit",
        )
    }

    #[test]
    fn the_names_come_from_the_app() {
        let f = photokit();
        assert_eq!(f.extension_id(), "com.example.photokit.context-menu");
        assert_eq!(f.helper_id(), "com.example.photokit.context-menu-helper");
        assert_eq!(f.group(), "AB12CD34EF.com.example.photokit.context-menu");
    }

    #[test]
    fn the_extension_is_finders() {
        let p = photokit().extension_plist();
        for want in [
            "\t<key>CFBundleDisplayName</key>\n\t<string>Photo &amp; Kit</string>\n",
            "\t<key>CFBundleIdentifier</key>\n\t<string>com.example.photokit.context-menu</string>\n",
            "\t<key>CFBundleExecutable</key>\n\t<string>context-menu-finder</string>\n",
            "\t<key>CFBundlePackageType</key>\n\t<string>XPC!</string>\n",
            "\t<key>ContextMenuRsMenu</key>\n\t<string>photo-kit</string>\n",
            "\t<key>CFBundleVersion</key>\n\t<string>1</string>\n",
            "\t<key>ContextMenuRsGroup</key>\n\t<string>AB12CD34EF.com.example.photokit.context-menu</string>\n",
            "\t\t<key>NSExtensionPointIdentifier</key>\n\t\t<string>com.apple.FinderSync</string>\n",
            "\t\t<key>NSExtensionPrincipalClass</key>\n\t\t<string>ContextMenuRsFinderSync</string>\n",
        ] {
            assert!(p.contains(want), "{want} in {p}");
        }
        let e = photokit().extension_entitlements();
        assert!(e.contains("<key>com.apple.security.app-sandbox</key>\n\t<true/>"));
        assert!(e.contains("<string>AB12CD34EF.com.example.photokit.context-menu</string>"));
        assert!(e.contains("<string>/Library/Application Support/context-menu-rs/</string>"));
    }

    #[test]
    fn the_helper_is_an_app_with_no_window() {
        let p = photokit().helper_plist();
        assert!(p.contains("<string>com.example.photokit.context-menu-helper</string>"));
        assert!(p.contains("\t<key>CFBundlePackageType</key>\n\t<string>APPL</string>\n"));
        assert!(p.contains("\t<key>LSBackgroundOnly</key>\n\t<true/>\n"));
        assert!(p.contains("\t<key>ContextMenuRsMenu</key>\n\t<string>photo-kit</string>\n"));
        assert!(!p.contains("NSExtension"));
        let e = photokit().helper_entitlements();
        assert!(e.contains("com.apple.security.application-groups"));
        assert!(!e.contains("app-sandbox"));
    }
}
