# Schemas

Versioned JSON Schemas for data that leaves a machine. Details and rules are in [../docs/SHARING.md](../docs/SHARING.md).

| Version | File | Status |
| --- | --- | --- |
| 1 | [v1/results.schema.json](v1/results.schema.json) | Draft until the first release; frozen after |

**The example file is illustrative.** Its effect values are made up to show the format, not measured results.

## Validating locally

```sh
pip install check-jsonschema
check-jsonschema --schemafile schema/v1/results.schema.json schema/v1/examples/*.json
```

The same check runs automatically on every pull request.
