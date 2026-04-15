.. _installation:

============
Installation
============

Topos requires Python 3.11 or newer.

Quick install
-------------

.. code-block:: bash

   curl -sSL https://raw.githubusercontent.com/Krv-Labs/topos/main/install.sh | sh

This downloads the latest release binary to ``~/.local/bin``.

From source
-----------

.. code-block:: bash

   git clone https://github.com/Krv-Labs/topos.git
   cd topos
   uv pip install -e .

Verify it works
---------------

.. code-block:: bash

   topos --help

Uninstall
---------

**Binary installer:**

.. code-block:: bash

   topos uninstall
   topos uninstall --dry-run
   topos uninstall --yes --prune-path-hints

**pip / uv:**

.. code-block:: bash

   uv pip uninstall topos
   # or
   pip uninstall topos

Optional: coupling analysis
---------------------------

Dependency-graph metrics (coupling, instability) require
`GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_, a separate npm tool
(Node.js 18+):

.. code-block:: bash

   npm install -g gitnexus
   gitnexus analyze          # run once in your repo root

This produces a ``.gitnexus/`` directory that ``--gitnexus-dir`` consumes.
