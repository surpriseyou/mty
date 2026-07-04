FROM mcr.microsoft.com/dotnet/sdk:10.0 AS build
WORKDIR /src

COPY server/Mty.Server.csproj server/
RUN dotnet restore server/Mty.Server.csproj

COPY server/ server/
RUN dotnet publish server/Mty.Server.csproj -c Release -o /app/publish --no-restore

FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS runtime
WORKDIR /app
COPY --from=build /app/publish ./
EXPOSE 5000
ENTRYPOINT ["dotnet", "Mty.Server.dll"]

