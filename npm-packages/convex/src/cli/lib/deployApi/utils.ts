/**
 * This object validator allows extra properties and passes them through.
 * This is useful for forwards compatibility if the server adds extra unknown
 * fields.
 */
export { looseObject } from "zod";
