.PHONY: all
all: ingestor notifier api

.PHONY: ingestor
ingestor: docker/job-ingestor.Dockerfile $(shell find job-ingestor -type f) $(shell find shared -type f)
	docker build --build-arg RUST_IMAGE=rust:1.87-slim -f docker/job-ingestor.Dockerfile -t job-ingestor .

.PHONY: notifier
notifier: docker/notifier.Dockerfile $(shell find notifier -type f) $(shell find shared -type f)
	docker build --build-arg RUST_IMAGE=rust:1.87-slim -f docker/notifier.Dockerfile -t notifier .

.PHONY: api
api: docker/notifier.Dockerfile $(shell find notifier -type f) $(shell find shared -type f)
	docker build --build-arg RUST_IMAGE=rust:1.87-slim -f docker/api.Dockerfile -t api .

.PHONY: clean
clean:
	docker image rm job-ingestor notifier api
