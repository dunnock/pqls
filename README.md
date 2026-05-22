# pqls

A command-line tool for listing the contents and metadata of Apache Parquet files and partitioned parquet datasets, modelled on HDF5's `h5ls`.

## Install

```sh
curl -fsSL https://github.com/dunnock/pqls/releases/latest/download/install.sh | sh
```

## Usage

```
pqls [OPTIONS] <PATH>

ARGS:
  <PATH>            file or directory

OPTIONS:
  -d, --detail      per-row-group stats, per-column min/max/nulls, partition layout
  -r, --recursive   recurse into subdirectories
      --csv         dump file contents as CSV to stdout
      --head <N>    with --csv, output only first N rows (0 = all)
  -q, --quiet       suppress decorative headers (machine-readable)
  -h, --help
  -V, --version
```

## Examples

**Inspect a single file:**
```sh
pqls data.parquet
```

**Detailed stats (per-column min/max/nulls):**
```sh
pqls -d data.parquet
```

**Dump as CSV:**
```sh
pqls --csv data.parquet
pqls --csv --head 100 data.parquet
```

**List a partitioned dataset:**
```sh
pqls /path/to/dataset/
pqls -d -r /path/to/dataset/
```

**Machine-readable output:**
```sh
pqls -q data.parquet
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
