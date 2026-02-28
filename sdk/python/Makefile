PROTO_DIR = ../../proto
OUT_DIR = dekode/_generated

.PHONY: proto
proto:
	python -m grpc_tools.protoc \
		-I$(PROTO_DIR) \
		--python_out=$(OUT_DIR) \
		--grpc_python_out=$(OUT_DIR) \
		--pyi_out=$(OUT_DIR) \
		dekode/v1/agent.proto \
		dekode/v1/types.proto
	# Fix relative imports in generated files
	sed -i.bak 's/from dekode.v1/from dekode._generated.dekode.v1/g' \
		$(OUT_DIR)/dekode/v1/agent_pb2_grpc.py \
		$(OUT_DIR)/dekode/v1/agent_pb2.py && rm -f $(OUT_DIR)/dekode/v1/*.bak || true

.PHONY: install
install:
	pip install -e ".[dev]"

.PHONY: test
test:
	pytest -v

.PHONY: clean
clean:
	rm -rf $(OUT_DIR)/dekode
