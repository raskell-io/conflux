FROM gcr.io/distroless/cc-debian12:nonroot

LABEL org.opencontainers.image.source="https://github.com/raskell-io/conflux"
LABEL org.opencontainers.image.description="Schema-aware config state coordination"
LABEL org.opencontainers.image.licenses="Apache-2.0"

COPY conflux /usr/local/bin/conflux

EXPOSE 9400

ENTRYPOINT ["/usr/local/bin/conflux"]
CMD ["daemon"]
