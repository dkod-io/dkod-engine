PROTO_DIR = ../../proto
OUT_DIR = dkod/_generated

.PHONY: proto
proto:
	python -m grpc_tools.protoc \
		-I$(PROTO_DIR) \
		--python_out=$(OUT_DIR) \
		--grpc_python_out=$(OUT_DIR) \
		--pyi_out=$(OUT_DIR) \
		dkod/v1/agent.proto \
		dkod/v1/types.proto
	# Fix relative imports in generated files
	sed -i.bak 's/from dkod.v1/from dkod._generated.dkod.v1/g' \
		$(OUT_DIR)/dkod/v1/agent_pb2_grpc.py \
		$(OUT_DIR)/dkod/v1/agent_pb2.py && rm -f $(OUT_DIR)/dkod/v1/*.bak || true

.PHONY: install
install:
	pip install -e ".[dev]"

.PHONY: test
test:
	pytest -v

.PHONY: clean
clean:
	rm -rf $(OUT_DIR)/dkod
