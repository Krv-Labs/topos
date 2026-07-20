import tomllib
from pathlib import Path

project = "Topos"
author = "Krv Labs"
copyright = "2026, Krv Labs"

# As of v0.4.0 (PR #159) Topos is an all-Rust workspace — there is no
# `topos` Python package to import __version__ from. Read the shared
# version straight from the workspace manifest instead, mirroring
# `scripts/check_versions.py`.
_root = Path(__file__).resolve().parent.parent.parent
with (_root / "Cargo.toml").open("rb") as _f:
    release = tomllib.load(_f)["workspace"]["package"]["version"]

extensions = [
    # No sphinx.ext.autodoc/autosummary/viewcode/napoleon: those support
    # Python API autodoc, and there is no Python API left to document.
    # Rust API docs live in rustdoc (`cargo doc`), not this Sphinx site —
    # see api.rst.
    "sphinx_design",
    "sphinx.ext.mathjax",
    "sphinx.ext.autosectionlabel",
    "sphinx.ext.todo",
    "sphinx.ext.githubpages",
    "sphinxcontrib.mermaid",
    "sphinx_copybutton",
]

autosectionlabel_prefix_document = True
autosectionlabel_maxdepth = 2

source_suffix = {
    ".rst": "restructuredtext",
    ".txt": "restructuredtext",
    ".md": "markdown",
}

templates_path = ["_templates"]
exclude_patterns = ["_build", "Thumbs.db", ".DS_Store"]

html_theme = "furo"
html_static_path = ["_static"]
html_extra_path = ["../../install.sh"]
html_title = "Topos Documentation"
html_css_files = ["custom.css"]

# Dark, muted code highlighting — dark code blocks on a white page (Variant B).
pygments_style = "github-dark"
pygments_dark_style = "github-dark"

# --- "Engineered" design system ---------------------------------------------
# White, high-contrast, monochrome base + a single ochre accent (Linear/Vercel
# discipline). Self-hosted Geist (see @font-face in custom.css). Tokens are
# theme-aware so Furo wires them to the right light/dark selectors and the
# manual theme toggle works correctly.
SANS = '"Geist","Geist Fallback",system-ui,-apple-system,"Segoe UI",Roboto,sans-serif'
MONO = '"Geist Mono","SF Mono",ui-monospace,Menlo,Consolas,monospace'

_light_vars = {
    # Brand accent — used sparingly (links, current nav, focus)
    "color-brand-primary": "#bf6209",
    "color-brand-content": "#bf6209",
    "color-brand-visited": "#9a5212",
    # Surfaces — true white, cool neutral grays
    "color-background-primary": "#ffffff",
    "color-background-secondary": "#f7f7f8",
    "color-background-hover": "#f2f2f3",
    "color-background-border": "#ededed",
    "color-foreground-primary": "#0a0a0a",
    "color-foreground-secondary": "#525252",
    "color-foreground-muted": "#8f8f8f",
    "color-foreground-border": "#ededed",
    # Sidebar
    "color-sidebar-background": "#ffffff",
    "color-sidebar-background-border": "#ededed",
    "color-sidebar-caption-text": "#8f8f8f",
    "color-sidebar-link-text": "#525252",
    "color-sidebar-link-text--top-level": "#0a0a0a",
    "color-sidebar-item-background--current": "#fdf3e7",
    "color-sidebar-item-background--hover": "#f7f7f8",
    "color-sidebar-search-border": "#ededed",
    "color-highlight-on-target": "#fdf3e7",
    # Dark code blocks even in light mode (Variant B)
    "color-code-background": "#0d1117",
    "color-code-foreground": "#e6edf3",
    "color-inline-code-background": "#f7f7f8",
    # Fonts
    "font-stack": SANS,
    "font-stack--monospace": MONO,
    # Topos semantic tokens (used by custom.css)
    "topos-bg": "#ffffff",
    "topos-soft": "#f7f7f8",
    "topos-line": "#ededed",
    "topos-line-strong": "#e2e2e2",
    "topos-ink": "#0a0a0a",
    "topos-ink-2": "#525252",
    "topos-ink-3": "#8f8f8f",
    "topos-accent": "#d9730d",
    "topos-accent-ink": "#bf6209",
    "topos-accent-soft": "#fdf3e7",
    "topos-code-bg": "#0d1117",
    # Figure plate (figures bake in their own ink-on-paper colors)
    "topos-plate-bg": "#ffffff",
    "topos-plate-border": "#ededed",
    "topos-image-outline": "rgba(0,0,0,0.08)",
    "topos-shadow-1": "rgba(16,24,40,0.04)",
    "topos-shadow-2": "rgba(16,24,40,0.08)",
    # Pillar / verdict colors (used only inside badges + figures)
    "topos-pillar-simple": "#2b7eb5",
    "topos-pillar-composable": "#126e5a",
    "topos-pillar-secure": "#c94040",
    "topos-gold": "#c9a25d",
    "topos-silver": "#9f988e",
    "topos-bronze": "#a96f45",
}

_dark_vars = {
    "color-brand-primary": "#f0a868",
    "color-brand-content": "#f0a868",
    "color-brand-visited": "#d6936a",
    "color-background-primary": "#0a0a0a",
    "color-background-secondary": "#141414",
    "color-background-hover": "#1c1c1c",
    "color-background-border": "#262626",
    "color-foreground-primary": "#ededed",
    "color-foreground-secondary": "#a3a3a3",
    "color-foreground-muted": "#737373",
    "color-foreground-border": "#262626",
    "color-sidebar-background": "#0a0a0a",
    "color-sidebar-background-border": "#1f1f1f",
    "color-sidebar-caption-text": "#737373",
    "color-sidebar-link-text": "#a3a3a3",
    "color-sidebar-link-text--top-level": "#ededed",
    "color-sidebar-item-background--current": "#2a1d0e",
    "color-sidebar-item-background--hover": "#1c1c1c",
    "color-sidebar-search-border": "#262626",
    "color-highlight-on-target": "#2a1d0e",
    "color-code-background": "#0d1117",
    "color-code-foreground": "#e6edf3",
    "color-inline-code-background": "#1c1c1c",
    "font-stack": SANS,
    "font-stack--monospace": MONO,
    "topos-bg": "#0a0a0a",
    "topos-soft": "#141414",
    "topos-line": "#262626",
    "topos-line-strong": "#333333",
    "topos-ink": "#ededed",
    "topos-ink-2": "#a3a3a3",
    "topos-ink-3": "#737373",
    "topos-accent": "#f0a868",
    "topos-accent-ink": "#f0a868",
    "topos-accent-soft": "#2a1d0e",
    "topos-code-bg": "#0d1117",
    "topos-plate-bg": "transparent",
    "topos-plate-border": "#262626",
    "topos-image-outline": "rgba(255,255,255,0.10)",
    "topos-shadow-1": "rgba(0,0,0,0.40)",
    "topos-shadow-2": "rgba(0,0,0,0.55)",
    "topos-pillar-simple": "#5aa6d8",
    "topos-pillar-composable": "#3fae8f",
    "topos-pillar-secure": "#e07a7a",
    "topos-gold": "#d8b878",
    "topos-silver": "#b5ad9f",
    "topos-bronze": "#c08a5c",
}

html_theme_options = {
    # Krv mark as backgroundless SVGs; Furo swaps them per theme via its native
    # only-light/only-dark. krv-logo-dark.svg is the same mark in a brighter red
    # for legibility on near-black. (CSS mask was cleaner in theory but Chrome
    # wouldn't honor the alpha mask for this SVG, so we use the proven <img> path.)
    "light_logo": "krv-logo.svg",
    "dark_logo": "krv-logo-dark.svg",
    "sidebar_hide_name": False,
    "light_css_variables": _light_vars,
    "dark_css_variables": _dark_vars,
    "source_repository": "https://github.com/Krv-Labs/topos",
    "source_branch": "main",
    "source_directory": "docs/source/",
    "footer_icons": [
        {
            "name": "Krv Labs",
            "url": "https://krv.ai",
            "html": "Built by Krv Labs →",
            "class": "topos-footer-krv",
        },
    ],
}
