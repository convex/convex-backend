import Markdown from "react-markdown";

import skills from "../data/agent-skills.json";

const SKILLS_BASE_URL =
  "https://github.com/convex/agent-skills/blob/main/skills";

export function AgentSkills() {
  return (
    <table>
      <thead>
        <tr>
          <th>Skill</th>
          <th>Description</th>
        </tr>
      </thead>
      <tbody>
        {skills.skills.map(({ name, description }) => (
          <tr key={name}>
            <td>
              <a href={`${SKILLS_BASE_URL}/${name}/SKILL.md`}>
                <code>/{name}</code>
              </a>
            </td>
            <td>
              {/* Unwrapping <p> keeps paragraph margins out of the table cell. */}
              <Markdown disallowedElements={["p"]} unwrapDisallowed>
                {description}
              </Markdown>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
