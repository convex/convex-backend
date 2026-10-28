## Running the binary directly

<details>
<summary>Getting the binary</summary>

You can either [build from source](../README.md) or use the precompiled
binaries. You can download the latest precompiled binary release from
[Releases](https://github.com/convex/convex-backend/releases). If your platform
is not supported, leave us a GitHub issue. In the meantime, you can build from
source.

_Note: On MacOS you might need to hold the `option` key and double click the
binary file in Finder once, to circumvent the
[Gatekeeper](https://support.apple.com/en-us/102445) warning._

</details>

<details>
<summary>Generate a new instance secret</summary>

The instance secret is the root secret for your backend. Anyone with this secret
can deploy code to and read from your deployment, so keep it safe and never
commit it to source control.

Generate a new random instance secret and store it in a shell variable for the
remaining steps:

```sh
export INSTANCE_SECRET=$(openssl rand -hex 32)
```

The result is a 64-character hex string. Rotating the secret will invalidate all
keys/sessions to your Convex backend.

</details>

<details>
<summary>Generate a new admin key</summary>

With the instance name and instance secret, generate an admin key. The admin key
is required to push code to the backend and take other administrator operations.

```sh
cargo run -p keybroker --bin generate_key -- convex-self-hosted "$INSTANCE_SECRET"
```

It will look like
`convex-self-hosted|01c046ab1512d9306a6abda3eedec5dfe862f1fe0f66a5aee774fb9ae3fda87706facaf682b9d4f9209a05e038cbd6e9b8`

</details>

<details>
<summary>Run your backend instance</summary>

Adjust the path based on where you downloaded the binary to or add it to your
`PATH`. The backend will store its database in the current-working-directory
(not where the binary file lives).

Use the instance name and the `$INSTANCE_SECRET` you generated above to start
your backend:

```sh
./convex-local-backend --instance-name convex-self-hosted --instance-secret "$INSTANCE_SECRET"
```

To run with Postgres, add `--db postgres-v5 <connection string>` to the command,
being sure to strip out the database name and query parameters. See
[Postgres instructions](#connecting-to-postgres-on-neon). To run with MySQL, add
`--db mysql-v5 <connection string>` to the command and similarly strip out the
database name and query parameters.

You can run `./convex-local-backend --help` to see other options for things like
changing ports, convex origin url, convex site url, local storage directories
and other configuration.

</details>
