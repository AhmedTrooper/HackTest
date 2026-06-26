# Build Stage
FROM node:22-slim AS builder
WORKDIR /app

# Install bun for building
RUN npm install -g bun

# Copy configuration and package details
COPY package.json bun.lock tsconfig.json tsr.config.json vite.config.ts biome.json ./
COPY src/ ./src/
COPY public/ ./public/

# Install dependencies and build TanStack Start bundle
RUN bun install
RUN bun run build

# Execution Stage
FROM node:22-slim
WORKDIR /app
COPY --from=builder /app/.output/ ./.output/

EXPOSE 3000
ENV PORT=3000
ENV NODE_ENV=production

CMD ["node", ".output/server/index.mjs"]
