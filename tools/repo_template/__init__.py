"""Repository template instantiation (spec node ECO.REPO.TPL.FILES). Standard library only.

``python -m repo_template new --id <repo-id> --variant code|spec --rules-source <dir>
--rules-rev <40-hex> --ci-rev <40-hex> --out <dir>`` (with ``tools/`` on the module path)
copies a variant of ``templates/repo/`` into a new folder, fills in the placeholders, writes
the house-rules blocks through the house-rules sync tool, and fails if any ``{{placeholder}}``
is left. See :mod:`repo_template.new`.
"""

from .new import TemplateError, instantiate, main

__all__ = ["TemplateError", "instantiate", "main"]
