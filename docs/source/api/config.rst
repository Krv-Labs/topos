Configuration
=============

Configuration modules handle runtime settings, repository discovery, and parser
support. These are useful when embedding Topos in another tool or debugging why
a file, project root, or optional parser dependency was not detected.

.. autosummary::
   :toctree: generated/config

   topos.config
   topos.utils.discovery
   topos.utils.tree_sitter
