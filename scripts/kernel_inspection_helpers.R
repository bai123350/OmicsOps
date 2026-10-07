inspect_bindings <- function(scope) {
    objects <- list(); truncated <- FALSE; budget <- 64L
    if (!is.environment(scope) || is.object(scope) || isS4(scope) || length(scope) > 4096L) return(list(objects=objects,truncated=TRUE))
    for (name in ls(envir=scope,all.names=TRUE,sorted=FALSE)) {
        if (length(objects) >= 64L) { truncated <- TRUE; break }
        if (nchar(name,type="chars") > 256L) { truncated <- TRUE; next }
        name <- enc2utf8(name)
        if (nchar(name,type="bytes") > 256L || grepl("[[:cntrl:]]",name) || startsWith(name,"_") ||
            grepl("password|passwd|secret|token|credential|private_key|api_key|apikey|access_key",tolower(name))) {
            truncated <- TRUE; next
        }
        if (bindingIsActive(name,scope)) { truncated <- TRUE; next }
        # substitute returns the promise expression or already bound object;
        # it never evaluates the expression or calls an active binding.
        value <- do.call(substitute,list(as.name(name),scope))
        label <- "opaque"; count <- NULL; dimensions <- NULL
        kind <- typeof(value)
        if (!is.object(value) && !isS4(value) &&
            kind %in% c("NULL","logical","integer","double","complex","character","raw","list")) {
            label <- kind; count <- length(value)
            raw_dims <- attr(value,"dim",exact=TRUE)
            if (!is.null(raw_dims) && !is.object(raw_dims) && !isS4(raw_dims) &&
                typeof(raw_dims) %in% c("integer","double") && length(raw_dims) <= 8L &&
                all(is.finite(raw_dims)) && all(raw_dims >= 0) && all(raw_dims == floor(raw_dims))) {
                dimensions <- as.list(raw_dims)
            }
        }
        item <- list(name=name,type_label=label,dimensions=dimensions,element_count=count,summary=paste0(label,", metadata only"))
        size <- nchar(enc2utf8(toJSON(item,auto_unbox=TRUE,null="null")),type="bytes") + 2L
        if (budget + size > 12288L) { truncated <- TRUE; break }
        objects[[length(objects)+1L]] <- item; budget <- budget + size
    }
    list(objects=objects,truncated=truncated)
}
