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

Install from Source
-------------------

.. code-block:: bash

   git clone https://github.com/Krv-Labs/topos.git
   cd topos
   uv pip install -e .

Verify Installation
-------------------

.. code-block:: bash

   topos --help
