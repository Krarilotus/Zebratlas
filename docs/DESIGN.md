# Design

Zebratlas helps people move from a rare-disease question to relevant research and an official next contact. Search results explain their match and retain links to the original source. The graph and query workspace make relationships inspectable without treating every connection as a proven scientific conclusion.

The data layer keeps source assertions, candidate identity matches and derived statements distinct. Record locators, hashes, activities and decision dependencies support rechecking and withdrawal. Licensing, privacy, accepted-identity rules and bounded query execution are separate gates.

The application uses a Rust API for indexed search, accounts, contributions and graph access, with a Next.js interface. Bounded RDF reasoning supports combined queries when the matching operational dataset and profile are provisioned. The included public sample demonstrates sourced condition–gene search and graph navigation; it carries no inference or full-corpus coverage claim.
