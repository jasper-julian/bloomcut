// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 BloomCut contributors

//! Which edition this build is: its name, where it keeps its files, and the
//! site it is downloaded from and updated by.
//!
//! One source, two editions. The default is BloomCut, published on
//! bloom.bloomaigc.com. Built with the `school` feature it is the school's
//! own edition, 「AI自动剪辑工具」, published on the school's site: a
//! different name everywhere a person reads one, its own folders - so the
//! two can be installed side by side - and its own update feed. The
//! interface's words name the product as "BloomCut", and
//! `concat::i18n` puts [`BRAND`]'s name in its place.

/// One edition.
#[derive(Debug)]
pub struct Brand {
    /// The product's name, as every window, dialog and file says it.
    pub name: &'static str,
    /// The folder name under %APPDATA% and %LOCALAPPDATA%.
    pub identifier: &'static str,
    /// Folders an earlier build of this edition kept its files under, to
    /// be moved to [`Brand::identifier`] on first start.
    pub former_identifiers: &'static [&'static str],
    /// The site it is published on.
    pub site: &'static str,
    /// The site's update feed, under [`Brand::site`].
    pub feed: &'static str,
    /// The site's download page, under [`Brand::site`].
    pub page: &'static str,
    /// The installer's file name once downloaded.
    pub setup_file: &'static str,
    /// How it names itself to a server: ASCII.
    pub agent: &'static str,
}

/// The edition this binary is.
#[cfg(not(feature = "school"))]
pub const BRAND: Brand = Brand {
    name: "BloomCut",
    identifier: "app.bloomcut.editor",
    former_identifiers: &["app.concat.editor", "org.standalones.concat"],
    site: "https://bloom.bloomaigc.com",
    feed: "/api/v1/bloom/bloomcut/latest",
    page: "/bloomcut",
    setup_file: "BloomCut-Setup.exe",
    agent: "BloomCut",
};

/// The edition this binary is: the school's.
#[cfg(feature = "school")]
pub const BRAND: Brand = Brand {
    name: "AI自动剪辑工具",
    identifier: "app.aiclip.editor",
    former_identifiers: &[],
    site: "https://115.190.195.41",
    feed: "/api/v1/bloom/desktop/latest",
    page: "/download",
    setup_file: "AI-Clip-Setup.exe",
    agent: "AIClip",
};

impl Brand {
    /// The update feed's whole address.
    pub fn feed_url(&self) -> String {
        format!("{}{}", self.site, self.feed)
    }

    /// The download page's whole address.
    pub fn page_url(&self) -> String {
        format!("{}{}", self.site, self.page)
    }

    /// `text` with the product named as this edition names it.
    pub fn named(&self, text: &str) -> String {
        if self.name == "BloomCut" {
            text.to_owned()
        } else {
            text.replace("BloomCut", self.name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BRAND;

    /// Nothing an edition shows a person names the other edition.
    #[test]
    fn an_edition_is_named_one_way() {
        assert!(BRAND.feed_url().starts_with("https://"));
        assert!(BRAND.page_url().starts_with(BRAND.site));
        let said = BRAND.named("BloomCut 0.3.0 is out");
        assert!(said.starts_with(BRAND.name));
        if cfg!(feature = "school") {
            for field in [
                BRAND.name,
                BRAND.identifier,
                BRAND.site,
                BRAND.page,
                BRAND.setup_file,
                BRAND.agent,
            ] {
                assert!(!field.to_lowercase().contains("bloom"), "{field}");
            }
        }
    }
}
