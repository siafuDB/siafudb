# 5. Relational data in a graph, and tabular projection

Reported, not built, as asked. Everything below ran against the `grafeo` 0.5.43 facade crate. The probe has no
lockfile, so `grafeo-engine` floated to **0.5.44** (see
[`probes/relational`](probes/relational/)). The repo's own lockfile pins 0.5.43;
results are unlikely to differ but were not re-run on it.

## Mapping

Row = labelled node. Foreign key = edge. Join table = edge with properties. Joins
become traversals. This needs nothing from SiafuDB beyond the engine.

## Tabular projection: what exists

| Language                 | Tabular / reporting result                                                                                                                                                                                       | Verified |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| GQL                      | Aggregates and `GROUP BY`-style returns with named columns: `RETURN p.city AS city, count(*) AS n, avg(p.age) AS avg_age` gave columns `[city, n, avg_age]`, 2 rows                                              | Yes      |
| Cypher                   | Same, with `ORDER BY`                                                                                                                                                                                            | Yes      |
| **SQL/PGQ**              | `SELECT * FROM GRAPH_TABLE (MATCH (p:Person) COLUMNS (p.name AS name, p.city AS city))` returned columns `[name, city]`, 3 rows. This **is** the SQL-style tabular projection. Called via `db.execute_sql(...)`. | Yes      |
| SPARQL, Gremlin, GraphQL | Available behind features. Not tested; irrelevant to reporting.                                                                                                                                                  | No       |

So a reporting layer needs no new language: SQL/PGQ `GRAPH_TABLE` gives rows and
columns an external tool can consume, and GQL/Cypher aggregates cover
dashboards. **Gotcha:** `SiafuDB` currently exposes only `execute` and `query`
(Cypher/GQL). It needs a way to select a language, and SiafuDB's workspace
features must enable `sql-pgq` (together with `gremlin` or `graphql`, because of
the upstream `VarGen` compile bug in 0.5.44 recorded in the README).

Weak spots, as expected: wide scans and large aggregates over the whole graph
are not where a graph engine shines; I did not benchmark them.

## Typed schema per label: what exists

Grafeo already enforces what the brief asks for. Verified:

| Need                           | Syntax that worked                                         | Result                                                                                                                                    |
| ------------------------------ | ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Required property + type check | `CREATE NODE TYPE Org (name STRING NOT NULL, founded INT)` | Insert without `name`: `Constraint violation: missing required property 'name' on :Org`. `founded:'notint'`: `expects Int64, got String`. |
| Uniqueness                     | `CREATE CONSTRAINT uq1 FOR (p:Person) ON (p.name) UNIQUE`  | Duplicate insert: `UNIQUE constraint violation: property 'name' with value "Amara" already exists on :Person`                             |
| Inspect                        | `SHOW CONSTRAINTS`                                         | Lists name, type, label, properties                                                                                                       |

Wrong guesses that failed, so nobody repeats them: `REQUIRE ... IS UNIQUE`,
`ON ... ASSERT ... IS UNIQUE`, and `UNIQUE` as a column modifier inside
`CREATE NODE TYPE`.

So SiafuDB's job is a thin, discoverable `Schema` builder that emits this DDL,
not a new constraint engine. Effort: days.

**Design conflict to resolve before relying on it:** a `UNIQUE` constraint is
local. Two offline devices can each insert `name: 'Amara'`; both succeed, and the
conflict appears at sync time. Uniqueness across a synced graph is therefore a
_sync-layer conflict rule_ (the `conflict` module), not a database constraint.
Required-property and type checks are safe to enforce locally; uniqueness needs a
merge policy. Strict relational guarantees (the brief's "strict constraints"
weak spot) hold per fragment, not globally.

Whether constraints survive `export_snapshot`/`import_snapshot` is likely (the
snapshot has a `schema` field) but _unverified_.

## KV injection

`SiafuDB::kv_set` escapes only `'`, replacing it with `\'`. A value ending in a
backslash-quote becomes `\\'`, which closes the string. Running the exact
escaping from `database.rs` against Grafeo with
`x\'}) WITH n MATCH (v:Victim) DETACH DELETE v //`:

```text
query: MERGE (n:_KV {key: 'k'}) SET n.value = 'x\\'}) WITH n MATCH (v:Victim) ...
result: Err(syntax error: unexpected '}' after the end of the statement)
```

The parser reached tokens that were attacker input, after the string should have
ended: the quote breakout is confirmed. My payload was malformed for this
statement shape, so it errored and the `Victim` node survived. I did not
construct a working destructive payload, so the severity is _confirmed breakout,
exploit not demonstrated_. `kv_get`, `kv_delete` and the document methods share
the pattern. Fix: Grafeo's parameterised queries (`execute_with_params`, `$name`
bindings, shown working in grafeo-web).

This also matters for sync: the injected text ends up in the change log, which
today is replayed verbatim on other devices.
