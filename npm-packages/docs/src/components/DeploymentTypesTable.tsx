// This table is defined in a .tsx file instead of
// multiple-deployments.mdx to avoid issues with
// MDX adding unwanted <p> tags inside of cell blocks
// and Prettier adding whitespace inside of `<code>`

export function DeploymentTypesTable() {
  return (
    <table>
      <caption>Default settings</caption>
      <thead>
        <tr>
          <th scope="row">Type</th>
          <th scope="col">
            <img
              src="/img/dtype_dev.png"
              width="116"
              height="23"
              alt="Dev"
              style={{ display: "block", margin: "auto" }}
            />
          </th>
          <th scope="col">
            <img
              src="/img/dtype_preview.png"
              width="86"
              height="23"
              alt="Preview"
              style={{ display: "block", margin: "auto" }}
            />
          </th>
          <th scope="col">
            <img
              src="/img/dtype_prod.png"
              width="103"
              height="23"
              alt="Production"
              style={{ display: "block", margin: "auto" }}
            />
          </th>
        </tr>
      </thead>
      <tbody>
        <tr>
          <th scope="row" align="left">
            Reference
          </th>
          <td>
            <code>
              dev/<em>[creator]</em>
            </code>
          </td>
          <td>
            <code>
              preview/<em>[branch]</em>
            </code>
          </td>
          <td>
            <code>production</code>
          </td>
        </tr>
        <tr>
          <th scope="row" align="left">
            Expiration
          </th>
          <td>—</td>
          <td>
            5 days <small>(Free and Starter plans)</small>
            <br />
            14 days{" "}
            <small>(Professional, Business, and Enterprise plans)</small>
          </td>
          <td>—</td>
        </tr>
        <tr>
          <th scope="row" align="left">
            Dashboard permissions
          </th>
          <td colSpan="2">Can be edited by every team member</td>
          <td>Can only be edited by project or team admins</td>
        </tr>
        <tr>
          <th scope="row" align="left">
            Server logs in clients
          </th>
          <td colSpan="2">Server logs are sent to the client</td>
          <td>
            Server logs are <em>not</em> sent to the client
          </td>
        </tr>
        <tr>
          <th scope="row" align="left">
            Server errors
          </th>
          <td colSpan="2">Details of server errors are sent to the client</td>
          <td>
            Details of server errors are <em>not</em> sent to the client (unless
            the error is wrapped in{" "}
            <a href="/functions/error-handling/application-errors#throwing-application-errors">
              <code>ConvexError</code>
            </a>
            )
          </td>
        </tr>
        <tr>
          <th scope="row" align="left">
            Dashboard edit confirmation
          </th>
          <td colSpan="2">No protection against accidental edits</td>
          <td>Asks for confirmation before editing</td>
        </tr>
      </tbody>
    </table>
  );
}
