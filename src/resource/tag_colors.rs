use crate::{
    client::PveClient,
    config::{AdoptionCandidate, ConfigDocument, LocalPatch, LocalState},
    reconcile::{ApiMethod, ApiTarget, Domain, Operation, PlanBuilder, ResourceId},
    utility::{property_string, yaml_patch::Segment},
};
use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) type TagColors = BTreeMap<TagName, TagColor>;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(transparent)]
pub(crate) struct TagName(#[schemars(regex(pattern = r"^[A-Za-z0-9_][A-Za-z0-9_+.-]*$"))] String);

impl std::borrow::Borrow<str> for TagName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for TagName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let mut bytes = value.bytes();
        if !bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || !bytes.all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'.' | b'-')
            })
        {
            return Err(de::Error::custom("invalid Proxmox tag name"));
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
pub(crate) struct HexColor(#[schemars(regex(pattern = r"^[0-9A-Fa-f]{6}$"))] String);

impl HexColor {
    pub(crate) fn rgb(&self) -> [u8; 3] {
        // Construction validates all six digits.
        [0, 2, 4].map(|offset| u8::from_str_radix(&self.0[offset..offset + 2], 16).unwrap())
    }
}

impl<'de> Deserialize<'de> for HexColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(de::Error::custom(
                "tag colors must be six hexadecimal digits without #",
            ));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TagColor {
    pub(crate) background: HexColor,
    /// Omit to let Proxmox choose a contrasting text color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<HexColor>,
}

/// The color map is owned separately from the remaining tag-style properties.
#[derive(Debug, Default)]
pub(crate) struct TagStyle {
    colors: TagColors,
    other: BTreeMap<String, String>,
}

impl TagStyle {
    pub(crate) fn into_colors(self) -> TagColors {
        self.colors
    }

    pub(crate) fn from_api(value: Option<&Value>) -> Result<Self> {
        let mut properties = match value {
            None => BTreeMap::new(),
            Some(Value::String(value)) => {
                let parsed = property_string::parse(value)?;
                if !parsed.positional.is_empty() {
                    bail!("tag-style requires named properties")
                }
                parsed
                    .keyed
                    .into_iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect()
            },
            Some(Value::Object(object)) => object
                .iter()
                .map(|(key, value)| {
                    let value = match value {
                        Value::String(value) => value.clone(),
                        Value::Bool(value) => u8::from(*value).to_string(),
                        Value::Number(value) => value.to_string(),
                        _ => bail!("invalid tag-style property {key}"),
                    };
                    Ok((key.clone(), value))
                })
                .collect::<Result<_>>()?,
            Some(_) => bail!("captured tag-style must be an object or property string"),
        };
        for (key, value) in &properties {
            if key.is_empty()
                || key.contains([',', '=', '\n', '\r'])
                || value.is_empty()
                || value.contains([',', '\n', '\r'])
            {
                bail!("invalid tag-style property {key}")
            }
        }
        let mut colors = TagColors::new();
        if let Some(map) = properties.remove("color-map") {
            for entry in map.split(';') {
                let parts = entry.split(':').collect::<Vec<_>>();
                let (tag, background, text) = match parts.as_slice() {
                    [tag, background] => (*tag, *background, None),
                    [tag, background, text] => (*tag, *background, Some(*text)),
                    _ => bail!("invalid tag color override {entry}"),
                };
                let tag: TagName = serde_json::from_value(Value::String(tag.into()))?;
                let color = TagColor {
                    background: serde_json::from_value(Value::String(background.into()))?,
                    text: text
                        .map(|text| serde_json::from_value(Value::String(text.into())))
                        .transpose()?,
                };
                if colors.insert(tag, color).is_some() {
                    bail!("duplicate tag color override in {map}")
                }
            }
        }
        Ok(Self {
            colors,
            other: properties,
        })
    }

    fn render(&self) -> Option<String> {
        let mut properties = self.other.clone();
        if !self.colors.is_empty() {
            properties.insert(
                "color-map".into(),
                self.colors
                    .iter()
                    .map(|(tag, color)| {
                        let mut value = format!("{}:{}", tag.0, color.background.0);
                        if let Some(text) = &color.text {
                            value.push(':');
                            value.push_str(&text.0);
                        }
                        value
                    })
                    .collect::<Vec<_>>()
                    .join(";"),
            );
        }
        (!properties.is_empty()).then(|| {
            properties
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(",")
        })
    }
}

pub(crate) fn captured_style(pve: &dyn PveClient) -> Result<TagStyle> {
    let options = pve
        .get("/cluster/options")
        .context("read captured cluster options; run capture again")?;
    if !options.is_object() {
        bail!("captured /cluster/options must be an object")
    }
    TagStyle::from_api(options.get("tag-style")).context("decode captured cluster tag-style")
}

pub(crate) fn plan(
    local: &LocalState,
    pve: &dyn PveClient,
    builder: &mut PlanBuilder,
) -> Result<()> {
    let Some(desired) = &local.cluster.tag_colors else {
        return Ok(());
    };
    let mut style = captured_style(pve)?;
    if &style.colors == desired {
        return Ok(());
    }
    let before = style.render();
    style.colors = desired.clone();
    let changes = match style.render() {
        Some(value) => BTreeMap::from([("tag-style".into(), value)]),
        None => BTreeMap::from([("delete".into(), "tag-style".into())]),
    };
    builder.operations().push(Operation::ApiMutation {
        target: ApiTarget::Pve,
        method: ApiMethod::Put,
        domain: Domain::Cluster,
        resource: ResourceId::Cluster,
        endpoint: "/cluster/options".into(),
        changes,
        before_values: BTreeMap::from([("tag-style".into(), before)]),
        environment_changes: BTreeMap::new(),
        digest: None,
    });
    Ok(())
}

pub(crate) fn candidates(
    local: &LocalState,
    pve: &dyn PveClient,
) -> Result<Vec<AdoptionCandidate>> {
    let style = captured_style(pve)?;
    if local.cluster.tag_colors.as_ref() == Some(&style.colors)
        || (local.cluster.tag_colors.is_none() && style.colors.is_empty())
    {
        return Ok(Vec::new());
    }
    Ok(vec![AdoptionCandidate::adoptable(
        &ResourceId::Cluster,
        "tag_colors",
        local
            .cluster
            .tag_colors
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?
            .unwrap_or_else(|| "(unmanaged)".into()),
        serde_json::to_string(&style.colors)?,
        vec![LocalPatch::ReplaceResource {
            document: ConfigDocument::Cluster,
            path: vec![Segment::Key("tag_colors".into())],
            value: serde_yaml::to_value(&style.colors)?,
        }],
    )])
}
