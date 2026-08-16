#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(hdf5r)
  library(Matrix)
  library(Seurat)
  library(SeuratObject)
  library(jsonlite)
})

args <- commandArgs(trailingOnly = TRUE)
if (length(args) != 3) {
  stop("usage: convert_h5ad_to_seurat.R INPUT.h5ad OUTPUT.rds VALIDATION.json")
}
input_path <- args[[1]]
output_path <- args[[2]]
validation_path <- args[[3]]

read_string_vector <- function(node) {
  value <- node[]
  if (is.raw(value)) {
    value <- rawToChar(value)
  }
  as.character(value)
}

read_index <- function(group) {
  index_name <- tryCatch(h5attr(group, "_index"), error = function(e) "_index")
  if (length(index_name) > 1) index_name <- index_name[[1]]
  read_string_vector(group[[as.character(index_name)]])
}

read_categorical <- function(group) {
  codes <- as.integer(group[["codes"]][])
  categories <- read_string_vector(group[["categories"]])
  values <- rep(NA_character_, length(codes))
  valid <- codes >= 0
  values[valid] <- categories[codes[valid] + 1L]
  factor(values, levels = categories)
}

read_dataframe <- function(group) {
  index <- read_index(group)
  frame <- data.frame(row.names = index)
  entries <- group$ls(recursive = FALSE)
  for (name in entries$name) {
    if (name == "_index") next
    node <- group[[name]]
    encoding <- tryCatch(as.character(h5attr(node, "encoding-type")), error = function(e) "")
    if (encoding == "categorical") {
      frame[[name]] <- read_categorical(node)
    } else if (inherits(node, "H5D")) {
      value <- node[]
      if (is.raw(value) || is.character(value)) value <- as.character(value)
      if (length(value) == nrow(frame)) frame[[name]] <- value
    }
  }
  frame
}

read_sparse_matrix <- function(group) {
  encoding <- as.character(h5attr(group, "encoding-type"))
  shape <- as.integer(h5attr(group, "shape"))
  data <- as.numeric(group[["data"]][])
  indices <- as.integer(group[["indices"]][])
  indptr <- as.integer(group[["indptr"]][])

  if (encoding == "csr_matrix") {
    counts_per_row <- diff(indptr)
    i <- rep(seq_len(shape[[1]]), counts_per_row)
    j <- indices + 1L
  } else if (encoding == "csc_matrix") {
    counts_per_col <- diff(indptr)
    i <- indices + 1L
    j <- rep(seq_len(shape[[2]]), counts_per_col)
  } else {
    stop(sprintf("unsupported sparse encoding: %s", encoding))
  }
  sparseMatrix(i = i, j = j, x = data, dims = shape, giveCsparse = TRUE)
}

read_matrix <- function(node) {
  if (inherits(node, "H5D")) {
    value <- if (length(node$dims) == 2) node[,] else node[]
    return(as(value, "dgCMatrix"))
  }
  read_sparse_matrix(node)
}

read_dense_matrix <- function(node) {
  if (!inherits(node, "H5D") || length(node$dims) != 2) {
    stop("expected a two-dimensional HDF5 dataset")
  }
  as.matrix(node[,])
}

read_embedding <- function(node, cell_names, prefix) {
  embedding <- read_dense_matrix(node)
  if (nrow(embedding) != length(cell_names) && ncol(embedding) == length(cell_names)) {
    embedding <- t(embedding)
  }
  if (nrow(embedding) != length(cell_names)) {
    stop(sprintf("%s embedding does not match the cell count", prefix))
  }
  rownames(embedding) <- cell_names
  colnames(embedding) <- paste0(prefix, seq_len(ncol(embedding)))
  embedding
}

h5 <- H5File$new(input_path, mode = "r")
on.exit(h5$close_all(), add = TRUE)

obs <- read_dataframe(h5[["obs"]])
var <- read_dataframe(h5[["var"]])
counts_node <- if (h5$exists("layers/counts")) h5[["layers/counts"]] else h5[["X"]]
cell_by_gene <- read_matrix(counts_node)
counts <- as(t(cell_by_gene), "dgCMatrix")
rownames(counts) <- rownames(var)
colnames(counts) <- rownames(obs)

seurat <- CreateSeuratObject(
  counts = counts,
  assay = "RNA",
  meta.data = obs,
  project = "OmicsOps"
)

if (h5$exists("obsm/X_pca")) {
  pca <- read_embedding(h5[["obsm/X_pca"]], rownames(obs), "PC_")
  seurat[["pca"]] <- CreateDimReducObject(
    embeddings = pca,
    key = "PC_",
    assay = DefaultAssay(seurat)
  )
}
if (h5$exists("obsm/X_umap")) {
  umap <- read_embedding(h5[["obsm/X_umap"]], rownames(obs), "UMAP_")
  seurat[["umap"]] <- CreateDimReducObject(
    embeddings = umap,
    key = "UMAP_",
    assay = DefaultAssay(seurat)
  )
}
if ("leiden" %in% colnames(seurat[[]])) {
  Idents(seurat) <- "leiden"
}

saveRDS(seurat, output_path, compress = "xz")
roundtrip <- readRDS(output_path)
checks <- list(
  cells = ncol(roundtrip) == nrow(obs),
  genes = nrow(roundtrip) == nrow(var),
  counts_nonzero = length(roundtrip[["RNA"]]$counts@x) == length(counts@x),
  metadata = all(rownames(roundtrip[[]]) == rownames(obs)),
  pca = !h5$exists("obsm/X_pca") || "pca" %in% Reductions(roundtrip),
  umap = !h5$exists("obsm/X_umap") || "umap" %in% Reductions(roundtrip),
  clusters = !"leiden" %in% colnames(obs) || all(as.character(Idents(roundtrip)) == as.character(obs$leiden)),
  annotation = !"cell_type" %in% colnames(obs) || all(as.character(roundtrip$cell_type) == as.character(obs$cell_type))
)
validation <- list(
  input = normalizePath(input_path),
  output = normalizePath(output_path),
  cells = ncol(roundtrip),
  genes = nrow(roundtrip),
  checks = checks,
  valid = all(unlist(checks))
)
write_json(validation, validation_path, auto_unbox = TRUE, pretty = TRUE)
if (!validation$valid) stop("h5ad to Seurat round-trip validation failed")
message(toJSON(validation, auto_unbox = TRUE, pretty = TRUE))
