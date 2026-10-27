FROM mcr.microsoft.com/dotnet/sdk:10.0 AS build
WORKDIR /source
COPY server/Fluxkit.Server/Fluxkit.Server.csproj server/Fluxkit.Server/
COPY server/Fluxkit.Server/packages.lock.json server/Fluxkit.Server/
RUN dotnet restore server/Fluxkit.Server/Fluxkit.Server.csproj --locked-mode
COPY server/Fluxkit.Server/ server/Fluxkit.Server/
RUN dotnet publish server/Fluxkit.Server/Fluxkit.Server.csproj -c Release --no-restore -o /app/publish

FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS runtime
WORKDIR /app
ENV ASPNETCORE_URLS=http://+:8080
EXPOSE 8080
COPY --from=build /app/publish .
USER $APP_UID
ENTRYPOINT ["dotnet", "Fluxkit.Server.dll"]
