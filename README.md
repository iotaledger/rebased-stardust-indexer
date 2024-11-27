# Indexer of stardust migration objects

A simple application indexing custom data from the stardust-migration objects.

## Setup

1. [Install Diesel CLI][diesel-getting-started]
2. Run `diesel setup`

## Supported features

* Index expiration unlock conditions for shared Nft and Basic outputs.
* Set a custom package defining the stardust outputs, assuming that the type
  layout is the same as in [iota-framework][].
* Expose a REST API to serve the indexed data.

[diesel-getting-started]: https://diesel.rs/guides/getting-started.html
