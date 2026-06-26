.PHONY: help api frontend docker api-run frontend-run docker-up docker-down docker-logs

# Default target
help:
	@echo "======================================================================"
	@echo "                     HACKATHON TEMPLATE MAKEFILE                      "
	@echo "======================================================================"
	@echo "Quick Commands:"
	@echo "  make api        - Build and run the Rust backend API locally (port 8080)"
	@echo "  make frontend   - Run the TanStack React Vite dev server (port 3000)"
	@echo "  make docker     - Spin up Postgres, Redis, and MinIO S3 containers"
	@echo ""
	@echo "Additional Commands:"
	@echo "  make docker-down - Stop Docker Compose containers"
	@echo "  make docker-logs - View real-time logs from local Docker Compose"
	@echo "======================================================================"

# Aliases matching your exact request
api: api-run
frontend: frontend-run
docker: docker-up

api-run:
	@echo "🚀 Starting Rust Axum backend on http://localhost:8080..."
	cd api && PORT=8080 RUST_LOG=info cargo run

frontend-run:
	@echo "💻 Starting TanStack React frontend on http://localhost:3000..."
	cd web && bun run dev

docker-up:
	@echo "🐳 Launching local infrastructure stack (Postgres, Redis, MinIO S3)..."
	docker compose up -d
	@echo "👀 Run 'make docker-logs' to monitor the database and service health."

docker-down:
	@echo "🛑 Tearing down Docker container stack..."
	docker compose down

docker-logs:
	docker compose logs -f
