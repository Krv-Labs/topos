use std::fmt::Write as _;

use super::model::{Badge, PillarRate, NOT_MEASURED_COLOR};

const HEIGHT: u32 = 20;
const LOGO_WIDTH: u32 = 26;
const CHIP_WIDTH: u32 = 28;
const METER_INSET: u32 = 4;
const METER_WIDTH: u32 = CHIP_WIDTH - 2 * METER_INSET;
const WIDTH: u32 = LOGO_WIDTH + 4 * CHIP_WIDTH;
const COLOR: &str = "#b45309";
const FONT: &str = "Verdana,Geneva,DejaVu Sans,sans-serif";
const LOGO: &str = include_str!("logo.svg");

/// Two-letter pillar code shown on the chip: `Si`, `Co`, `Se`, `Na`.
fn code(rate: &PillarRate) -> String {
    let name = rate.pillar.as_str();
    format!("{}{}", name[..1].to_ascii_uppercase(), &name[1..2])
}

/// Filled length of a chip's meter, in tenths of a pixel.
fn meter_tenths(rate: u8) -> u32 {
    METER_WIDTH * 10 * u32::from(rate) / 100
}

pub(crate) fn render(badge: &Badge) -> String {
    let label = format!("topos: {}", badge.summary().replace(" · ", ", "));
    let (mut chips, mut meters, mut codes) = (String::new(), String::new(), String::new());
    for (index, rate) in badge.rates().iter().enumerate() {
        let x = LOGO_WIDTH + CHIP_WIDTH * index as u32;
        let fill = rate.rate().map_or(NOT_MEASURED_COLOR, |_| COLOR);
        write!(
            chips,
            r#"
    <rect x="{x}" width="{CHIP_WIDTH}" height="{HEIGHT}" fill="{fill}"/>"#
        )
        .expect("writing to String cannot fail");
        if index > 0 {
            write!(
                chips,
                r##"
    <rect x="{x}" y="3" width="1" height="14" fill="#000" fill-opacity=".18"/>"##
            )
            .expect("writing to String cannot fail");
        }
        if let Some(rate) = rate.rate() {
            let start = x + METER_INSET;
            let filled = meter_tenths(rate);
            write!(
                meters,
                r##"
  <rect x="{start}" y="16" width="{METER_WIDTH}" height="2" rx="1" fill="#fff" fill-opacity=".3"/>
  <rect x="{start}" y="16" width="{}.{}" height="2" rx="1" fill="#fff"/>"##,
                filled / 10,
                filled % 10
            )
            .expect("writing to String cannot fail");
        }
        let center = x + CHIP_WIDTH / 2;
        write!(
            codes,
            r#"
    <text x="{center}" y="13">{}</text>"#,
            code(rate)
        )
        .expect("writing to String cannot fail");
    }
    let chips_width = WIDTH - LOGO_WIDTH;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" role="img" aria-label="{label}">
  <title>{label}</title>
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
  <clipPath id="r"><rect width="{WIDTH}" height="{HEIGHT}" rx="3" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="{LOGO_WIDTH}" height="{HEIGHT}" fill="#fff"/>{chips}
    <rect x="{LOGO_WIDTH}" width="{chips_width}" height="{HEIGHT}" fill="url(#s)"/>
  </g>{meters}
  <rect x=".5" y=".5" width="{}" height="19" rx="2.5" fill="none" stroke="{COLOR}" stroke-width="1"/>
  {LOGO}
  <g fill="#fff" text-anchor="middle" font-family="{FONT}" font-size="11" font-weight="bold">{codes}
  </g>
</svg>
"##,
        WIDTH - 1
    )
}

#[cfg(test)]
mod tests {
    use super::super::model::tests::badge_with;
    use super::*;

    const ALL: [usize; 4] = [40, 40, 40, 40];

    fn assert_well_formed(svg: &str) {
        let mut open: Vec<&str> = Vec::new();
        let mut rest = svg;
        while let Some(start) = rest.find('<') {
            let end = start + rest[start..].find('>').expect("unterminated tag");
            let tag = &rest[start + 1..end];
            rest = &rest[end + 1..];
            if let Some(name) = tag.strip_prefix('/') {
                assert_eq!(open.pop(), Some(name), "mismatched </{name}>");
            } else if !tag.ends_with('/') {
                open.push(tag.split_whitespace().next().expect("empty tag"));
            }
            assert_eq!(tag.matches('"').count() % 2, 0, "unbalanced quotes: {tag}");
        }
        assert!(open.is_empty(), "unclosed tags: {open:?}");
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.ends_with("</svg>\n"));
    }

    fn meter_fills(svg: &str) -> Vec<&str> {
        svg.lines()
            .filter(|line| line.contains(r##"y="16""##) && line.ends_with(r##"fill="#fff"/>"##))
            .filter_map(|line| line.split("width=\"").nth(1)?.split('"').next())
            .collect()
    }

    #[test]
    fn every_kind_of_badge_is_well_formed_svg() {
        for badge in [
            badge_with(ALL, ALL),
            badge_with([1, 2, 0, 3], ALL),
            badge_with([39, 0, 36, 30], [40, 0, 40, 40]),
            badge_with([0; 4], [0; 4]),
        ] {
            let svg = render(&badge);
            assert_well_formed(&svg);
            assert!(svg.contains(r#"width="138" height="20" role="img""#));
        }
    }

    #[test]
    fn chips_show_pillar_codes_in_order_and_the_title_carries_the_rates() {
        let svg = render(&badge_with([39, 0, 36, 30], [40, 0, 40, 40]));
        let positions: Vec<usize> = [">Si<", ">Co<", ">Se<", ">Na<"]
            .iter()
            .map(|text| svg.find(text).unwrap_or_else(|| panic!("missing {text}")))
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let label = "topos: SIMPLE 97%, COMPOSABLE n/a, SECURE 90%, NAVIGABLE 75%";
        assert!(svg.contains(&format!(r#"aria-label="{label}""#)));
        assert!(svg.contains(&format!("<title>{label}</title>")));
    }

    #[test]
    fn meters_fill_in_proportion_to_each_rate() {
        let svg = render(&badge_with([39, 0, 36, 30], [40, 0, 40, 40]));
        assert_eq!(meter_fills(&svg), ["19.4", "18.0", "15.0"]);
        assert_eq!(meter_fills(&render(&badge_with(ALL, ALL))), ["20.0"; 4]);
        assert_eq!(meter_fills(&render(&badge_with([0; 4], ALL))), ["0.0"; 4]);
    }

    #[test]
    fn an_unmeasured_pillar_is_grey_with_no_meter() {
        let svg = render(&badge_with([39, 0, 36, 30], [40, 0, 40, 40]));
        assert!(svg.contains(&format!(
            r#"<rect x="54" width="28" height="20" fill="{NOT_MEASURED_COLOR}"/>"#
        )));
        assert!(!svg.contains(r#"<rect x="58" y="16""#));
        assert_eq!(svg.matches(&format!(r#"fill="{COLOR}"/>"#)).count(), 3);
    }
}
