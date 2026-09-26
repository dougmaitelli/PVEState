use crate::{
    client::PveClient,
    config::LocalState,
    resource::tag_colors::{self, TagColors, TagStyle},
};
use anyhow::Result;

#[derive(Default)]
pub(crate) struct TagPalettes {
    pub(super) captured: TagColors,
    pub(super) local: TagColors,
}

impl TagPalettes {
    pub(crate) fn load(local: &LocalState, pve: &dyn PveClient) -> Result<Self> {
        let captured = tag_colors::captured_style(pve)?.into_colors();
        Ok(Self {
            local: local
                .cluster
                .tag_colors
                .clone()
                .unwrap_or_else(|| captured.clone()),
            captured,
        })
    }
}

pub(super) fn display_value(field: &str, value: &str, palette: &TagColors, colors: bool) -> String {
    if !colors || value.is_empty() {
        return super::display_value(field, value);
    }
    match field {
        "tags" => value
            .split(';')
            .map(|tag| paint(tag, palette))
            .collect::<Vec<_>>()
            .join(";"),
        "tag-style" => {
            let Ok(style) = TagStyle::from_api(Some(&serde_json::Value::String(value.into())))
            else {
                return value.into();
            };
            let palette = style.into_colors();
            value
                .split(',')
                .map(|property| {
                    let Some(map) = property.strip_prefix("color-map=") else {
                        return property.into();
                    };
                    let entries = map
                        .split(';')
                        .map(|entry| {
                            let Some((tag, color)) = entry.split_once(':') else {
                                return entry.into();
                            };
                            format!("{}:{color}", paint(tag, &palette))
                        })
                        .collect::<Vec<_>>()
                        .join(";");
                    format!("color-map={entries}")
                })
                .collect::<Vec<_>>()
                .join(",")
        },
        _ => super::display_value(field, value),
    }
}

fn paint(tag: &str, palette: &TagColors) -> String {
    if tag.is_empty() {
        return String::new();
    }
    let override_color = palette.get(tag);
    let background =
        override_color.map_or_else(|| default_background(tag), |color| color.background.rgb());
    let foreground = override_color
        .and_then(|color| color.text.as_ref())
        .map_or_else(|| contrasting_text(background), |color| color.rgb());
    let [r, g, b] = background;
    let [fr, fg, fb] = foreground;
    console::style(tag)
        .on_true_color(r, g, b)
        .true_color(fr, fg, fb)
        .force_styling(true)
        .to_string()
}

// Match Proxmox's generated palette and automatic text contrast:
// https://github.com/proxmox/proxmox-widget-toolkit/blob/master/src/Utils.js
fn default_background(tag: &str) -> [u8; 3] {
    let hash = tag
        .encode_utf16()
        .chain("prox".encode_utf16())
        .fold(0_u32, |hash, character| {
            hash.wrapping_mul(31).wrapping_add(u32::from(character))
        });
    [0, 8, 16].map(|shift| (((hash >> shift) & 255) as f64 * 0.7 + 76.5).round() as u8)
}

fn contrasting_text([r, g, b]: [u8; 3]) -> [u8; 3] {
    let luminance = (f64::from(r) / 255.0).powf(2.4) * 0.2126729
        + (f64::from(g) / 255.0).powf(2.4) * 0.7151522
        + (f64::from(b) / 255.0).powf(2.4) * 0.072175;
    let clamped = if luminance > 0.022 {
        luminance
    } else {
        luminance + (0.022 - luminance).powf(1.414)
    };
    if (clamped.powf(0.65) - 1.0).abs() >= (clamped.powf(0.56) - 0.046134502).abs() {
        [255; 3]
    } else {
        [0; 3]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct CapturedPalette;

    impl PveClient for CapturedPalette {
        fn endpoint(&self) -> &str {
            "https://pve.test:8006"
        }
        fn get(&self, path: &str) -> Result<serde_json::Value> {
            assert_eq!(path, "/cluster/options");
            Ok(json!({"tag-style": {"color-map": "web:ff0000"}}))
        }
        fn put(&self, _: &str, _: &std::collections::BTreeMap<String, String>) -> Result<()> {
            unreachable!()
        }
        fn post(&self, _: &str, _: &std::collections::BTreeMap<String, String>) -> Result<()> {
            unreachable!()
        }
        fn delete(&self, _: &str, _: &std::collections::BTreeMap<String, String>) -> Result<()> {
            unreachable!()
        }
    }

    #[test]
    fn local_palette_distinguishes_unmanaged_cleared_and_overridden_colors() {
        let temp = tempfile::tempdir().unwrap();
        crate::config::scaffold::initialize(temp.path()).unwrap();
        let mut local = crate::config::open(temp.path()).unwrap();
        let unmanaged = TagPalettes::load(&local, &CapturedPalette).unwrap();
        assert_eq!(unmanaged.captured, unmanaged.local);
        local.cluster.tag_colors = Some(TagColors::new());
        let cleared = TagPalettes::load(&local, &CapturedPalette).unwrap();
        assert_eq!(cleared.captured["web"].background.rgb(), [255, 0, 0]);
        assert!(cleared.local.is_empty());
        assert_eq!(default_background("web"), [210, 191, 250]);
        assert_eq!(default_background("production"), [191, 177, 239]);
        assert!(paint("web", &cleared.local).contains("\x1b[48;2;210;191;250m"));
        local.cluster.tag_colors =
            Some(serde_json::from_value(json!({"web": {"background": "0000ff"}})).unwrap());
        let overridden = TagPalettes::load(&local, &CapturedPalette).unwrap();
        assert_eq!(overridden.captured["web"].background.rgb(), [255, 0, 0]);
        assert_eq!(overridden.local["web"].background.rgb(), [0, 0, 255]);
    }

    #[test]
    fn explicit_colors_and_automatic_contrast_are_rendered() {
        let palette: TagColors = serde_json::from_value(json!({
            "custom": {"background": "123456", "text": "abcdef"},
            "dark": {"background": "000000"},
            "light": {"background": "ffffff"}
        }))
        .unwrap();
        let rendered = display_value("tags", "custom;dark;light", &palette, true);
        assert!(rendered.contains("\x1b[38;2;171;205;239m\x1b[48;2;18;52;86mcustom\x1b[0m"));
        assert!(rendered.contains("\x1b[38;2;255;255;255m\x1b[48;2;0;0;0mdark\x1b[0m"));
        assert!(rendered.contains("\x1b[38;2;0;0;0m\x1b[48;2;255;255;255mlight\x1b[0m"));
        assert_eq!(console::strip_ansi_codes(&rendered), "custom;dark;light");
    }

    #[test]
    fn plain_values_and_non_tag_fields_remain_unchanged() {
        for (field, value) in [
            ("tags", "web;db"),
            ("tag-style", "color-map=web:123456,shape=full"),
            ("description", "web;db"),
        ] {
            assert_eq!(display_value(field, value, &TagColors::new(), false), value);
        }
        assert_eq!(
            display_value("description", "web;db", &TagColors::new(), true),
            "web;db"
        );
        assert_eq!(
            display_value("tags", "", &TagColors::new(), true),
            "(empty)"
        );
        assert_eq!(contrasting_text([0; 3]), [255; 3]);
        assert_eq!(contrasting_text([255; 3]), [0; 3]);
    }

    #[test]
    fn tag_style_uses_its_own_colors_and_preserves_hex_values() {
        let value = "color-map=web:123456:abcdef;db:000000,shape=full";
        let rendered = display_value("tag-style", value, &TagColors::new(), true);
        assert!(rendered.contains("\x1b[48;2;18;52;86mweb\x1b[0m:123456:abcdef"));
        assert!(rendered.contains("\x1b[48;2;0;0;0mdb\x1b[0m:000000"));
        assert_eq!(console::strip_ansi_codes(&rendered), value);
    }
}
