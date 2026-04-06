.. _installation:

============
Installation
============

Topos requires Python 3.11 or newer.

Quick Install
-------------

.. code-block:: bash

   curl -sSL https://raw.githubusercontent.com/Krv-Labs/topos/main/install.sh | sh

This downloads the latest release binary and installs it to ``~/.local/bin``.

Uninstall (binary installer):

.. code-block:: bash

   topos uninstall
   topos uninstall --dry-run
   topos uninstall --yes --prune-path-hints

Install from Source
-------------------

.. code-block:: bash

   git clone https://github.com/Krv-Labs/topos.git
   cd topos
   uv pip install -e .

Uninstall (package manager):

.. code-block:: bash

   uv pip uninstall topos
   # or
   pip uninstall topos

Verify Installation
-------------------

.. code-block:: bash

   topos --help
