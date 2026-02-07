# /install-deps - Install Dependencies

## Trigger

`/install-deps`

## Description

Installs dependencies for all projects.

## Actions

- `dotnet restore` - Restores NuGet packages
- `npm install` - Installs Node dependencies

## Subprojects

If monorepo, runs in all configured subprojects.
