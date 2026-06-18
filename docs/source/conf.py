import os
import sys

sys.path.insert(0, os.path.abspath("../../src"))
sys.path.insert(0, os.path.abspath("../.."))

project = "Topos"
author = "Krv Labs"
copyright = "2026, Krv Labs"

from topos import __version__  # noqa: E402

release = __version__

extensions = [
    "sphinx.ext.autodoc",
    "sphinx.ext.viewcode",
    "sphinx.ext.intersphinx",
    "sphinx_design",
    "sphinx.ext.napoleon",
    "sphinx.ext.mathjax",
    "sphinx.ext.autosectionlabel",
    "sphinx.ext.todo",
    "sphinx.ext.githubpages",
    "sphinxcontrib.mermaid",
    "sphinx_copybutton",
]

autosectionlabel_prefix_document = True
autosectionlabel_maxdepth = 2

autodoc_default_options = {
    "members": True,
    "undoc-members": True,
    "show-inheritance": True,
}

source_suffix = {
    ".rst": "restructuredtext",
    ".txt": "restructuredtext",
    ".md": "markdown",
}

templates_path = ["_templates"]
exclude_patterns = ["_build", "Thumbs.db", ".DS_Store"]

intersphinx_mapping = {
    "python": ("https://docs.python.org/3", None),
}

html_theme = "furo"
html_static_path = ["_static"]
html_extra_path = ["../../install.sh"]
html_title = "Topos Documentation"
html_css_files = ["custom.css"]

html_theme_options = {
    "light_logo": "logo.png",
    "dark_logo": "logo-dark.png",
    "sidebar_hide_name": False,
    "footer_icons": [
        {
            "name": "Krv Labs",
            "url": "https://krv.ai",
            "html": "Built by Krv Labs →",
            "class": "",
        }
    ],
}
