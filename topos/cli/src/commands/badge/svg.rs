use std::fmt::Write as _;

use topos_engine::core::omega::Generator;

use super::model::{Badge, PillarState};

const ICON: &str = include_str!("icon.svg");
const LABEL_COLOR: &str = "#ffffff";
const FAIL_COLOR: &str = "#78716c";
const NOT_MEASURED_COLOR: &str = "#e7e5e4";
const NOT_MEASURED_TEXT: &str = "#57534e";
const BADGE_FONT: &str = "Verdana,Geneva,DejaVu Sans,sans-serif";
const EMOJI_FONT: &str = "Apple Color Emoji, Segoe UI Emoji, Noto Color Emoji, sans-serif";
const GRADIENT: &str = r##"  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
"##;

const CHIP_WIDTH: u32 = 28;
const LATTICE_HEIGHT: u32 = 20;
const LATTICE_PLATE: u32 = 26;
const VERDICT_HEIGHT: u32 = 24;
const VERDICT_PLATE: u32 = 26;
const VERDICT_PANEL: u32 = 28;

struct ChipPaint {
    fill: &'static str,
    text: &'static str,
    opacity: &'static str,
}

fn chip_paint(state: PillarState, medal_color: &'static str) -> ChipPaint {
    match state {
        PillarState::Pass => ChipPaint {
            fill: medal_color,
            text: "#fff",
            opacity: "1",
        },
        PillarState::Fail => ChipPaint {
            fill: FAIL_COLOR,
            text: "#fff",
            opacity: "0.85",
        },
        PillarState::NotMeasured => ChipPaint {
            fill: NOT_MEASURED_COLOR,
            text: NOT_MEASURED_TEXT,
            opacity: "1",
        },
    }
}

fn abbreviation(pillar: Generator) -> String {
    let name = pillar.as_str();
    format!("{}{}", name[..1].to_ascii_uppercase(), &name[1..2])
}

fn icon(size: u32, x: u32, y: u32) -> String {
    format!(
        r#"<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="0 0 240 240" aria-hidden="true" focusable="false" preserveAspectRatio="xMidYMid meet">{ICON}</svg>"#
    )
}

fn open_svg(width: u32, height: u32, label: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" role="img" aria-label="{label}">
  <title>{label}</title>
{GRADIENT}"#
    )
}

fn chips(badge: &Badge) -> (String, String, String) {
    let (mut rects, mut separators, mut labels) = (String::new(), String::new(), String::new());
    for (index, (pillar, state)) in badge.pillars().iter().enumerate() {
        let x = LATTICE_PLATE + CHIP_WIDTH * index as u32;
        let paint = chip_paint(*state, badge.color());
        write!(
            rects,
            r#"<rect x="{x}" width="{CHIP_WIDTH}" height="{LATTICE_HEIGHT}" fill="{}"/>"#,
            paint.fill
        )
        .expect("writing to String cannot fail");
        if index > 0 {
            write!(
                separators,
                r##"<rect x="{x}" y="3" width="1" height="14" fill="#000" fill-opacity=".18"/>"##
            )
            .expect("writing to String cannot fail");
        }
        write!(
            labels,
            r#"<text x="{}" y="14" fill="{}" fill-opacity="{}">{}</text>"#,
            x + CHIP_WIDTH / 2,
            paint.text,
            paint.opacity,
            abbreviation(*pillar)
        )
        .expect("writing to String cannot fail");
    }
    (rects, separators, labels)
}

pub(crate) fn render_lattice(badge: &Badge) -> String {
    let total = LATTICE_PLATE + CHIP_WIDTH * badge.pillars().len() as u32;
    let label = format!("topos: {} ({})", badge.medal(), badge.pillar_summary());
    let (rects, separators, labels) = chips(badge);
    format!(
        r##"{open}  <clipPath id="r"><rect width="{total}" height="{LATTICE_HEIGHT}" rx="3" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="{LATTICE_PLATE}" height="{LATTICE_HEIGHT}" fill="{LABEL_COLOR}"/>
    {rects}
    {separators}
    <rect x="{LATTICE_PLATE}" width="{chips_width}" height="{LATTICE_HEIGHT}" fill="url(#s)"/>
  </g>
  <rect x=".5" y=".5" width="{inner_width}" height="19" rx="2.5" fill="none" stroke="{color}" stroke-width="1"/>
  {icon}
  <g text-anchor="middle" font-family="{BADGE_FONT}" font-size="11" font-weight="bold">
    {labels}
  </g>
</svg>
"##,
        open = open_svg(total, LATTICE_HEIGHT, &label),
        chips_width = total - LATTICE_PLATE,
        inner_width = total - 1,
        color = badge.color(),
        icon = icon(16, 5, 2),
    )
}

pub(crate) fn render_verdict(badge: &Badge) -> String {
    let total = VERDICT_PLATE + VERDICT_PANEL;
    let label = format!("Topos lattice verdict: {}", badge.medal());
    format!(
        r##"{open}  <clipPath id="r"><rect width="{total}" height="{VERDICT_HEIGHT}" rx="3.2" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="{VERDICT_PLATE}" height="{VERDICT_HEIGHT}" fill="{LABEL_COLOR}"/>
    <rect x="{VERDICT_PLATE}" width="{VERDICT_PANEL}" height="{VERDICT_HEIGHT}" fill="{color}"/>
    <rect x="{VERDICT_PLATE}" y="4" width="1" height="16" fill="#000" fill-opacity=".18"/>
    <rect x="{VERDICT_PLATE}" width="{VERDICT_PANEL}" height="{VERDICT_HEIGHT}" fill="url(#s)"/>
  </g>
  <rect x=".5" y=".5" width="{inner_width}" height="{inner_height}" rx="3.2" fill="none" stroke="{color}" stroke-width="1"/>
  {icon}
  <text x="{medal_x}" y="17" text-anchor="middle" font-family="{EMOJI_FONT}" font-size="14">{emoji}</text>
</svg>
"##,
        open = open_svg(total, VERDICT_HEIGHT, &label),
        color = badge.color(),
        inner_width = total - 1,
        inner_height = VERDICT_HEIGHT - 1,
        icon = icon(14, 6, 5),
        medal_x = VERDICT_PLATE + VERDICT_PANEL / 2,
        emoji = badge.emoji(),
    )
}

#[cfg(test)]
mod tests {
    use super::super::model::tests::{badge, badge_passing};
    use super::*;

    fn reference(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/source/_static")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
    }

    fn without_label(svg: &str) -> String {
        svg.lines()
            .filter(|line| !line.contains("<title>"))
            .map(|line| match line.find(" aria-label=\"") {
                Some(start) if line.starts_with("<svg xmlns") => format!("{}>", &line[..start]),
                _ => line.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn platinum_lattice_matches_the_committed_badge_apart_from_its_label() {
        assert_eq!(
            without_label(&render_lattice(&badge_passing(4))),
            without_label(&reference("topos-lattice-badge.svg")),
            "the Rust port drifted from scripts/generate_self_badge.py; regenerate the reference or fix svg.rs"
        );
    }

    #[test]
    fn platinum_verdict_matches_the_committed_badge_apart_from_its_label() {
        assert_eq!(
            without_label(&render_verdict(&badge_passing(4))),
            without_label(&reference("topos-verdict.svg")),
            "the Rust port drifted from scripts/generate_self_badge.py; regenerate the reference or fix svg.rs"
        );
    }

    #[test]
    fn each_tier_colors_passing_chips_and_mutes_failed_ones() {
        for (count, medal, color, emoji) in [
            (4, "PLATINUM", "#b45309", "🏆"),
            (3, "GOLD", "#b45309", "🥇"),
            (2, "SILVER", "#525252", "🥈"),
            (1, "BRONZE", "#92400e", "🥉"),
            (0, "NO MEDAL", "#a3a3a3", "❌"),
        ] {
            let badge = badge_passing(count);
            let lattice = render_lattice(&badge);
            let chip = |fill: &str| {
                lattice
                    .matches(&format!(r#"height="20" fill="{fill}"/>"#))
                    .count()
            };
            assert_eq!(chip(color), count, "{medal} passing chips");
            assert_eq!(chip(FAIL_COLOR), 4 - count, "{medal} failed chips");
            assert!(lattice.contains(&format!(r#"aria-label="topos: {medal} ("#)));
            assert!(lattice.contains(&format!(r#"stroke="{color}""#)));

            let verdict = render_verdict(&badge);
            assert!(verdict.contains(&format!(r#"aria-label="Topos lattice verdict: {medal}""#)));
            assert!(verdict.contains(emoji), "{medal} emoji");
            assert!(verdict.contains(&format!(r#"height="24" fill="{color}"/>"#)));
        }
    }

    #[test]
    fn an_unmeasured_pillar_gets_its_own_chip_and_label() {
        let svg = render_lattice(&badge(
            &[Generator::Simple, Generator::Secure, Generator::Navigable],
            &[],
        ));
        assert_eq!(
            svg.matches(&format!(r#"fill="{NOT_MEASURED_COLOR}"/>"#))
                .count(),
            1
        );
        assert!(!svg.contains(&format!(r#"fill="{FAIL_COLOR}"/>"#)));
        assert!(svg.contains(&format!(
            r#"<text x="68" y="14" fill="{NOT_MEASURED_TEXT}" fill-opacity="1">Co</text>"#
        )));
        assert!(svg.contains("COMPOSABLE not measured"));
    }

    #[test]
    fn chips_are_abbreviated_in_pillar_order() {
        let svg = render_lattice(&badge_passing(4));
        let positions: Vec<usize> = ["Si", "Co", "Se", "Na"]
            .iter()
            .map(|abbr| svg.find(&format!(">{abbr}</text>")).expect(abbr))
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
