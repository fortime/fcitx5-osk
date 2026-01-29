use std::{path::PathBuf, result::Result as StdResult};

use getset::Getters;
use iced::{
    theme::{
        palette::{Background, Danger, Extended, Pair, Primary, Secondary, Success},
        Palette, Theme as IcedTheme,
    },
    Color,
};
use serde::{
    de::{Error, Unexpected},
    Deserialize, Deserializer,
};

use crate::store::IdAndConfigPath;

#[derive(Deserialize)]
struct RawTheme {
    name: String,
    palette: RawPalette,
    extended_palette: Option<RawExtendedPalette>,
}

#[derive(Deserialize)]
struct RawPalette {
    background: String,
    text: String,
    primary: String,
    success: String,
    danger: String,
}

#[derive(Deserialize)]
struct RawExtendedPalette {
    background: RawExtendedPaletteColorSet,
    primary: RawExtendedPaletteColorSet,
    secondary: RawExtendedPaletteColorSet,
    success: RawExtendedPaletteColorSet,
    danger: RawExtendedPaletteColorSet,
    is_dark: bool,
}

#[derive(Deserialize)]
struct RawExtendedPaletteColorSet {
    base: RawColorPair,
    weak: RawColorPair,
    strong: RawColorPair,
}

#[derive(Deserialize)]
struct RawColorPair {
    color: String,
    text: String,
}

#[derive(Getters)]
pub(crate) struct Theme {
    path: Option<PathBuf>,
    #[getset(get = "pub")]
    name: String,
    #[getset(get = "pub")]
    iced_theme: IcedTheme,
}

impl IdAndConfigPath for Theme {
    type IdType = String;

    fn id(&self) -> &Self::IdType {
        &self.name
    }

    fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    fn set_path<T: Into<PathBuf>>(&mut self, path: T) {
        self.path = Some(path.into());
    }
}

impl<'de> Deserialize<'de> for Theme {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let RawTheme {
            name,
            palette,
            extended_palette,
        } = Deserialize::deserialize(deserializer)?;

        let palette = Palette {
            background: parse_color(&palette.background)?,
            text: parse_color(&palette.text)?,
            primary: parse_color(&palette.primary)?,
            success: parse_color(&palette.success)?,
            danger: parse_color(&palette.danger)?,
        };

        let iced_theme = if let Some(extended) = extended_palette {
            let extended = Extended {
                background: Background {
                    base: parse_color_pair(&extended.background.base)?,
                    weak: parse_color_pair(&extended.background.weak)?,
                    strong: parse_color_pair(&extended.background.strong)?,
                },
                primary: Primary {
                    base: parse_color_pair(&extended.primary.base)?,
                    weak: parse_color_pair(&extended.primary.weak)?,
                    strong: parse_color_pair(&extended.primary.strong)?,
                },
                secondary: Secondary {
                    base: parse_color_pair(&extended.secondary.base)?,
                    weak: parse_color_pair(&extended.secondary.weak)?,
                    strong: parse_color_pair(&extended.secondary.strong)?,
                },
                success: Success {
                    base: parse_color_pair(&extended.success.base)?,
                    weak: parse_color_pair(&extended.success.weak)?,
                    strong: parse_color_pair(&extended.success.strong)?,
                },
                danger: Danger {
                    base: parse_color_pair(&extended.danger.base)?,
                    weak: parse_color_pair(&extended.danger.weak)?,
                    strong: parse_color_pair(&extended.danger.strong)?,
                },
                is_dark: extended.is_dark,
            };

            IcedTheme::custom_with_fn(name.clone(), palette, |_| extended)
        } else {
            IcedTheme::custom(name.clone(), palette)
        };

        Ok(Theme {
            path: None,
            name,
            iced_theme,
        })
    }
}

fn parse_color<E>(color: &str) -> StdResult<Color, E>
where
    E: Error,
{
    Color::parse(color).ok_or_else(|| {
        E::invalid_value(
            Unexpected::Str(color),
            &"`#rrggbb`, `#rrggbbaa`, `#rgb`, or `#rgba`",
        )
    })
}

fn parse_color_pair<E>(pair: &RawColorPair) -> StdResult<Pair, E>
where
    E: Error,
{
    Ok(Pair {
        color: parse_color(&pair.color)?,
        text: parse_color(&pair.text)?,
    })
}
