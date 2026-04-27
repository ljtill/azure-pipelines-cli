# Authentication

`devops` uses the Azure SDK [`DeveloperToolsCredential`](https://learn.microsoft.com/azure/developer/intro/azure-developer-cli), which tries local developer credentials in this order:

1. Azure CLI (`az login`)
2. Azure Developer CLI (`azd auth login`)

For local development, ensure you're logged in with one of these:

```sh
az login
# or
azd auth login
```

The first credential that successfully acquires a token is used. No PATs or service principals are read from the config file or environment — sign in with the CLI and `devops` picks up the credentials transparently.
