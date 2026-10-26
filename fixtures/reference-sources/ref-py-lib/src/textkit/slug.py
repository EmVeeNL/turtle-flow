def slugify(value: str) -> str:
    """Return a basic lowercase, hyphen-separated slug."""
    return "-".join(value.lower().split())
