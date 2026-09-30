FROM node:22-alpine AS web-build
WORKDIR /src/web

COPY web/package*.json ./
RUN npm ci

COPY web/ ./
ARG VITE_API_BASE_URL=/
ENV VITE_API_BASE_URL=${VITE_API_BASE_URL}
RUN npm run build

FROM mcr.microsoft.com/dotnet/sdk:10.0 AS server-build
WORKDIR /src

COPY server/Mty.Server.csproj server/
RUN dotnet restore server/Mty.Server.csproj

COPY server/ server/
RUN dotnet publish server/Mty.Server.csproj -c Release -o /app/publish --no-restore
COPY --from=web-build /src/web/dist/ /app/publish/wwwroot/

FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS runtime
WORKDIR /app
COPY --from=server-build /app/publish ./
COPY web/docker-entrypoint.sh /docker-entrypoint.sh
RUN chmod +x /docker-entrypoint.sh

ENV WEB_ROOT=/app/wwwroot
ENV VITE_API_BASE_URL=/
EXPOSE 5000
ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["dotnet", "Mty.Server.dll"]
