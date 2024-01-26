# fluree-migrate

`fluree-migrate` is a CLI tool to help migrate Fluree v2 ledge data to Fluree v3.

## Installation

Ensure you have `cargo` installed on your machine. You can install `cargo` by following the instructions [here](https://doc.rust-lang.org/cargo/getting-started/installation.html).

> On Linux and MacOs machines, simply run the following command in your terminal:
>
> ```bash
> curl https://sh.rustup.rs -sSf | sh
> ```

Once you have `cargo` installed, you can install `fluree-migrate` by running the following command in your terminal:

```bash
cargo install --git https://github.com/fluree/fluree-migrate
```

## Usage

The `fluree-migrate` tool can be used by simply running `fluree-migrate` in your terminal or by running `fluree-migrate` with several useful flags and options.

If just using `fluree-migrate` without any flags or options, the tool will prompt you for the URL of the existing Fluree v2 ledger to migrate from (and, if this is hosted on Fluree's Cloud platform, then it will prompt you for an API Key with which to access that ledger).

Note the following about the default behavior of `fluree-migrate` without any options or flags:

- The tool will generate a JSON-LD representation of the Fluree v2 schema with RDF/RDFS terms (this is purely metadata for your own reference)
- The tool will migrate your v2 data to v3 JSON-LD data and will write this to a local directory path that defaults to `output/` (this can be configured via the --output flag)
- The tool will default to IRI prefixes based on the URL of your existing v2 ledger (e.g. if your v2 ledger is hosted at `http://flur.ee/ledger/example`, then the tool will default to IRI prefixes of `http://flur.ee/ledger/example/ids/` and `http://flur.ee/ledger/example/terms/` for data and vocab entities, respectively -- this can be updated via the --base and --vocab flags)

- The tool **will not** generate a set of SHACL shapes to enforce schema validation for your JSON-LD data (this requires the use of the --shacl flag)
- The tool **will not** attempt to transact the migrated data to an existing v3 ledger (this requires the use of the --target flag)

## Flags & Options

### Flags

The following flags are available for use with `fluree-migrate`:

#### `--shacl`

This flag will cause the tool to generate a set of SHACL shapes to enforce schema validation for your JSON-LD data. If writing the data to a local directory, the resulting SHACL shapes will be written to the same file as the vocab metadata (e.g. `0_vocab.jsonld`)

#### `--closed-shapes`

This flag will cause the tool to generate "closed" SHACL shapes (i.e. no additional properties can be added to instances of the class).

This flag is only useful if the `--shacl` flag is also used.

#### `--print`

This flag will cause the tool to print the output to stdout instead of writing to local files or to a target v3 instance.

This flag conflicts with the `--output` and `--target` flags.

#### `--create-ledger`

This flag will cause the tool to attempt to create the ledger on the target v3 instance. Only use this flag if you have not yet created a v3 ledger to transact your data into.

This flag is only useful if the `--target` flag is also used.

### Options

#### `--output`

This option is used to specify the relative path to the directory where the output files will be written. If a value is not provided on `--output`, then the tool will default to writing the output to a directory named `output/` in the current working directory.

Writing to a local directory is the default behavior of the tool. The alternatives are to print the output to stdout (`--print`) or to transact the output to a target v3 instance (`--target`).

#### `--source`

This option is used to specify the URL of the existing Fluree v2 ledger to migrate from. If this is hosted on Fluree's Cloud platform, then you will also need to provide an API Key with which to access that ledger.

If a value is not provided on `--source`, then the tool will prompt you for this URL regardless.

#### `--source-auth`

This option is used to specify an API Key with which to access the existing Fluree v2 ledger to migrate from. This is only necessary if the ledger is hosted on Fluree's Cloud platform.

If a value is not provided on `--source-auth`, then the tool will prompt you for this API Key if it receives a 401 response from the ledger.

#### `--target`

This option is used to specify the URL of the target v3 Fluree instance to transact the migrated data to. It is an alternative to using `--output` to write the data to local files or to using `--print` to print the data to stdout.

#### `--target-auth`

This option is used to specify an API Key with which to access the target v3 Fluree instance. This is only necessary if the target v3 instance is hosted on Fluree's Cloud platform.

If a value is not provided on `--target-auth`, then the tool will prompt you for this API Key if it receives a 401 response from the target v3 instance.

#### `--base`

This option is used to specify the `@base` value for the @context of the output JSON-LD. This will be used as a default IRI prefix for all data entities (e.g. `http://example.org/ids/`).

If a value is not provided on `--base`, then the tool will default to using the URL of the existing v2 ledger as the prefix for the `@base` value.

#### `--vocab`

This option is used to specify the `@vocab` value for the @context of the output JSON-LD. This will be used as a default IRI prefix for all vocab entities (e.g. `http://example.org/terms/`).

If a value is not provided on `--vocab`, then the tool will default to using the URL of the existing v2 ledger as the prefix for the `@vocab` value.

## Additional Help

The following is the output of `fluree-migrate --help`:

```bash
fluree-migrate 0.1.0
Converts Fluree v2 schema JSON to Fluree v3 JSON-LD

USAGE:
    fluree-migrate [FLAGS] [OPTIONS]

FLAGS:
        --closed-shapes    This depends on the --shacl flag being used. If set, then the resulting SHACL shapes will be "closed" (i.e. no additional properties can be added to instances of the class)

    -h, --help             Prints help information

        --create-ledger    This depends on the --target flag being used. If set, then the first transaction issued against the target will attempt to create the ledger

        --print            If set, then the output will be printed to stdout instead of written to local files or to a target v3 instance. [Conflicts with --output & --target]

        --shacl            If set, then the result vocab JSON-LD will include SHACL shapes for each class

    -V, --version          Prints version information

OPTIONS:
    -b, --base <base>                  @base value for @context. This will be used as a default IRI prefix for all data entities. e.g. http://example.org/ids/

    -o, --output <output>              If writing the output to local files, then this is the relative path to the directory where the files will be written. [Conflicts with --target & --print]                           [default: output]

    -s, --source <source>              Accessible URL for v2 Fluree DB. This will be used to fetch the schema and data state

        --source-auth <source-auth>    Authorization token for Nexus ledgers. e.g. 796b******854d

    -t, --target <target>              If transacting the output to a target v3 Fluree instance, this is the URL for that instance. e.g. http://localhost:58090 [Conflicts with --output & --print]

        --target-auth <target-auth>    Authorization token for the target v3 instance (if hosted on Nexus). Only useful if transacting the output to a target v3 Fluree instance

    -v, --vocab <vocab>                @vocab value for @context. This will be used as a default IRI prefix for all vocab entities. e.g. http://example.org/terms/
```
