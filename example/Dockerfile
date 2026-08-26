FROM rust:1.98-bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p example-function

FROM gcr.io/distroless/cc-debian12@sha256:e5d81ddde149641e2a9ba55be4545bc125c67de07508b03ba4c22e6eb0ded5aa AS image
COPY --from=build /src/target/release/example-function /usr/local/bin/function
EXPOSE 9443
USER nonroot:nonroot
ENTRYPOINT ["function"]
