.. _installation:

============
Installation
============

Topos requires Python 3.11 or newer.

Quick Install
-------------

.. code-block:: bash

   curl -sSL https://raw.githubusercontent.com/Krv-Labs/topos/main/install.sh | bash

This downloads the latest release binary and installs it to ``~/.local/bin``.

Install as a Python Package
---------------------------

.. code-block:: bash

   # Using uv (recommended)
   uv pip install topos

   # Using pip
   pip install topos

Install from Source
-------------------

.. code-block:: bash

   git clone https://github.com/Krv-Labs/topos.git
   cd topos
   uv sync

Verify Installation
-------------------

.. code-block:: bash

   topos --help
