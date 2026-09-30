# Predicate Vocabulary

A predicate names the relationship an edge represents — the verb in `(subject, predicate, object)`
(spec section 20). `Predicate` is deliberately **an open string type, not a closed enum**
(`crates/oag-core/src/predicate.rs`): spec section 20 explicitly rejects building a universal
ontology up front. Any predicate string may be asserted; the vocabulary below documents the v0 set
this project's own crawler/CLI/tests use, not an exhaustive or enforced list.

## Canonicalization

`Predicate::new` normalizes any input the same way regardless of source: trim whitespace, lowercase,
collapse internal whitespace to underscores. `"Implements"`, `" implements "`, and `"implements"` all
canonicalize to the same predicate, so two independent contributors phrasing the same relationship
slightly differently still produce the same edge.

## The v0 vocabulary (`KNOWN_PREDICATES`)

| Predicate | Typical meaning |
|---|---|
| `official_source` | The object is the authoritative source for the subject |
| `documentation_for` | The subject documents the object |
| `authored_by` | The subject was authored by the object |
| `published_by` | The subject was published by the object |
| `owned_by` | The subject is owned by the object |
| `operated_by` | The subject is operated by the object |
| `maintained_by` | The subject is maintained by the object |
| `implements` | The subject implements the object (a spec, protocol, or interface) |
| `supports` | The subject supports the object (a feature, protocol, format) |
| `does_not_support` | The subject explicitly does not support the object |
| `requires` | The subject requires the object to function |
| `depends_on` | The subject depends on the object |
| `compatible_with` | The subject is compatible with the object |
| `exposes` | The subject exposes the object (an API, capability) |
| `references` | The subject references the object |
| `links_to` | The subject links to the object |
| `derived_from` | The subject is derived from the object |
| `source_for` | The subject is the source for the object |
| `cites` | The subject cites the object |
| `describes` | The subject describes the object |
| `explains` | The subject explains the object |
| `example_of` | The subject is an example of the object |
| `instance_of` | The subject is an instance of the object (used as the baseline fact for every crawled page — see `crates/oag-crawler/src/crawler.rs`) |
| `part_of` | The subject is part of the object |
| `version_of` | The subject is a version of the object |
| `supersedes` | The subject supersedes the object |
| `deprecated_by` | The subject is deprecated by the object |
| `replaced_by` | The subject is replaced by the object |
| `supports_claim` | The subject supports the claim represented by the object |
| `contradicts` | The subject contradicts the object |
| `disputes` | The subject disputes the object |
| `alternative_to` | The subject is an alternative to the object |
| `related_to` | Generic, weakest-typed relatedness — use a more specific predicate when one applies |
| `provides_capability` | The subject provides the capability named by the object (used by ARD/A2A extraction) |
| `uses_protocol` | The subject uses the protocol named by the object |
| `exposed_via` | The subject is exposed via the mechanism named by the object |
| `download_at` | The object is a download location for the subject |
| `repository_at` | The object is the source repository for the subject |
| `documented_at` | The object is documentation for the subject |
| `homepage` | The object is the subject's homepage |

## Adding a predicate

There is no registration step. Assert an edge with whatever predicate string best describes the
relationship — `Predicate::new` will canonicalize it, and every downstream consumer (search, ranking,
the human interface) works over the raw predicate string, not a fixed enum. If a predicate you need
recurs often enough to be worth documenting for other contributors, add it to `KNOWN_PREDICATES` and
this table; that's a documentation change, not a schema or code change.
