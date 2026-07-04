FROM node:22-alpine AS build
WORKDIR /app

COPY web/package*.json ./
RUN npm ci

COPY web/ ./
RUN npm run build

FROM caddy:latest AS runtime
ENV VITE_API_BASE_URL=http://localhost:5000
COPY web/Caddyfile /etc/caddy/Caddyfile
COPY web/docker-entrypoint.sh /docker-entrypoint.sh
COPY --from=build /app/dist /usr/share/nginx/html
RUN chmod +x /docker-entrypoint.sh
EXPOSE 80
ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["caddy", "run", "--config", "/etc/caddy/Caddyfile", "--adapter", "caddyfile"]
