# agent/schemas.py
"""OpenAI-style tool schemas exposed to the model.

Schemas are derived automatically from the tool registry so they never drift
from the actual implementations.
"""
from tools import registry

TOOLS = registry.schemas()
