use std::fmt::Write as _;

use super::model::Badge;

const LABEL: &str = "topos";
const LABEL_COLOR: &str = "#555";
const HEIGHT: u32 = 20;
const PADDING: u32 = 10;
const FONT: &str = "Verdana,Geneva,DejaVu Sans,sans-serif";

const NARROW: [(char, f32); 10] = [
    (' ', 3.5),
    ('.', 3.5),
    ('/', 4.7),
    ('I', 4.6),
    ('J', 5.0),
    ('f', 3.9),
    ('i', 3.0),
    ('j', 3.3),
    ('l', 3.0),
    ('t', 4.3),
];
const WIDE: [(char, f32); 7] = [
    ('%', 11.9),
    ('M', 9.3),
    ('O', 8.7),
    ('W', 10.9),
    ('m', 10.7),
    ('w', 8.9),
    ('D', 8.5),
];
const DEFAULT_WIDTH: f32 = 7.6;
const LOWERCASE_WIDTH: f32 = 6.6;

pub(crate) fn text_width(text: &str) -> u32 {
    let width: f32 = text
        .chars()
        .map(|ch| {
            let fallback = if ch.is_ascii_lowercase() {
                LOWERCASE_WIDTH
            } else {
                DEFAULT_WIDTH
            };
            NARROW
                .iter()
                .chain(WIDE.iter())
                .find(|(known, _)| *known == ch)
                .map_or(fallback, |(_, width)| *width)
        })
        .sum();
    width.ceil() as u32
}

struct Segment {
    text: String,
    fill: &'static str,
    x: u32,
    width: u32,
}

fn segments(badge: &Badge) -> Vec<Segment> {
    let texts = std::iter::once((LABEL.to_string(), LABEL_COLOR)).chain(
        badge
            .rates()
            .iter()
            .map(|rate| (format!("{} {}", rate.name(), rate.shown()), rate.color())),
    );
    let mut x = 0;
    texts
        .map(|(text, fill)| {
            let width = text_width(&text) + PADDING;
            let segment = Segment {
                text,
                fill,
                x,
                width,
            };
            x += width;
            segment
        })
        .collect()
}

pub(crate) fn render(badge: &Badge) -> String {
    let segments = segments(badge);
    let total: u32 = segments.iter().map(|segment| segment.width).sum();
    let label = format!("topos: {}", badge.summary().replace(" · ", ", "));
    let (mut rects, mut texts) = (String::new(), String::new());
    for (index, segment) in segments.iter().enumerate() {
        let Segment {
            text,
            fill,
            x,
            width,
        } = segment;
        write!(
            rects,
            r#"
    <rect x="{x}" width="{width}" height="{HEIGHT}" fill="{fill}"/>"#
        )
        .expect("writing to String cannot fail");
        if index > 1 {
            write!(
                rects,
                r##"
    <rect x="{x}" width="1" height="{HEIGHT}" fill="#000" fill-opacity=".2"/>"##
            )
            .expect("writing to String cannot fail");
        }
        let center = (2 * x + width) * 5;
        let length = (width - PADDING) * 10;
        write!(
            texts,
            r##"
    <text x="{center}" y="150" transform="scale(.1)" fill="#010101" fill-opacity=".3" textLength="{length}" lengthAdjust="spacingAndGlyphs" aria-hidden="true">{text}</text>
    <text x="{center}" y="140" transform="scale(.1)" textLength="{length}" lengthAdjust="spacingAndGlyphs">{text}</text>"##
        )
        .expect("writing to String cannot fail");
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="{HEIGHT}" role="img" aria-label="{label}">
  <title>{label}</title>
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
  <clipPath id="r"><rect width="{total}" height="{HEIGHT}" rx="3" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">{rects}
    <rect width="{total}" height="{HEIGHT}" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="{FONT}" text-rendering="geometricPrecision" font-size="110">{texts}
  </g>
</svg>
"##
    )
}

#[cfg(test)]
mod tests {
    use super::super::model::tests::badge_with;
    use super::super::model::NOT_MEASURED_COLOR;
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

    fn fills(svg: &str) -> Vec<&str> {
        svg.lines()
            .filter(|line| {
                line.contains(r##"height="20" fill="#"##) && !line.contains(r#"width="1""#)
            })
            .filter_map(|line| line.split("fill=\"").nth(1)?.split('"').next())
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
            assert_well_formed(&render(&badge));
        }
    }

    #[test]
    fn segments_spell_out_each_pillar_and_its_rate_in_order() {
        let svg = render(&badge_with([39, 0, 36, 30], [40, 0, 40, 40]));
        let positions: Vec<usize> = [
            ">topos<",
            ">SIMPLE 97%<",
            ">COMPOSABLE n/a<",
            ">SECURE 90%<",
            ">NAVIGABLE 75%<",
        ]
        .iter()
        .map(|text| svg.find(text).unwrap_or_else(|| panic!("missing {text}")))
        .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            fills(&svg),
            [
                LABEL_COLOR,
                "#97ca00",
                NOT_MEASURED_COLOR,
                "#a4a61d",
                "#dfb317"
            ]
        );
        let label = "topos: SIMPLE 97%, COMPOSABLE n/a, SECURE 90%, NAVIGABLE 75%";
        assert!(svg.contains(&format!(r#"aria-label="{label}""#)));
        assert!(svg.contains(&format!("<title>{label}</title>")));
    }

    #[test]
    fn all_high_and_all_low_badges_take_the_ends_of_the_scale() {
        assert_eq!(&fills(&render(&badge_with(ALL, ALL)))[1..], ["#44cc11"; 4]);
        assert_eq!(
            &fills(&render(&badge_with([0; 4], ALL)))[1..],
            ["#e05d44"; 4]
        );
    }

    #[test]
    fn segments_are_wide_enough_for_their_text_and_never_overlap() {
        let badge = badge_with([39, 0, 36, 30], [40, 0, 40, 40]);
        let segments = segments(&badge);
        assert_eq!(segments[0].x, 0);
        for pair in segments.windows(2) {
            assert_eq!(pair[0].x + pair[0].width, pair[1].x, "{}", pair[1].text);
        }
        for segment in &segments {
            assert!(
                segment.width >= text_width(&segment.text) + PADDING,
                "{}",
                segment.text
            );
        }
        assert!(text_width("NAVIGABLE 100%") > text_width("NAVIGABLE 9%"));
        assert!(text_width("COMPOSABLE") > text_width("SIMPLE"));
        let total: u32 = segments.iter().map(|segment| segment.width).sum();
        assert!(render(&badge).contains(&format!(r#"width="{total}" height="20" role="img""#)));
    }

    #[test]
    fn text_is_squeezed_into_its_measured_width() {
        let svg = render(&badge_with(ALL, ALL));
        let width = text_width("SECURE 100%");
        assert!(svg.contains(&format!(
            r#"textLength="{}" lengthAdjust="spacingAndGlyphs">SECURE 100%</text>"#,
            width * 10
        )));
    }
}
