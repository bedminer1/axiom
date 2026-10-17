# syntax=docker/dockerfile:1.7
FROM rust:1.88-slim-bookworm AS rust-builder
ENV CARGO_BUILD_JOBS=1
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY prompts ./prompts
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --locked --release --bin axiom-datadog && \
    cp target/release/axiom-datadog /tmp/axiom-datadog

FROM python:3.12-slim-bookworm AS python-builder
RUN --mount=type=cache,target=/root/.cache/pip \
    pip install --no-compile --prefix=/install \
      'python-docx>=1.1,<2' 'Pillow>=10'

FROM python:3.12-slim-bookworm AS runtime
ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    REPORT_PYTHON=/usr/local/bin/python3 \
    REPORT_SCRIPT=/opt/datadog/report.py \
    BOT_DATA_DIR=/var/lib/datadog \
    YOLO_MODEL=/opt/datadog/models/crack.onnx \
    ORT_DYLIB_PATH=/opt/datadog/models/libonnxruntime.so

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates libstdc++6 && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --system --gid 10001 datadog && \
    useradd --system --uid 10001 --gid datadog --home-dir /nonexistent datadog && \
    mkdir -p /opt/datadog/models /var/lib/datadog && \
    chown -R datadog:datadog /var/lib/datadog

COPY --from=rust-builder /tmp/axiom-datadog /usr/local/bin/axiom-datadog
COPY --from=python-builder /install /usr/local
COPY report.py /opt/datadog/report.py
COPY models/crack.onnx models/libonnxruntime.so models/libonnxruntime_providers_shared.so /opt/datadog/models/

USER 10001:10001
WORKDIR /opt/datadog
VOLUME ["/var/lib/datadog"]
ENTRYPOINT ["/usr/local/bin/axiom-datadog"]
