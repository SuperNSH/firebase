// Parsing and validation of a Remote Config template: the REST
// `RemoteConfig` resource shape (conditions, parameters,
// parameterGroups, version), read from the v1 discovery document of
// firebaseremoteconfig.googleapis.com. Mirrors
// crates/nsh-firebase-remote-config/src/template.rs rule for rule.

import { FirebaseError } from "../core/error";
import { RC_CODES } from "./codes";
import type {
  ParsedCondition,
  ParsedParameter,
  ParsedRemoteConfigTemplate,
} from "./types";

function invalid(message: string): FirebaseError {
  return new FirebaseError(RC_CODES.invalidTemplate, message);
}

function asObject(value: unknown, what: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw invalid(`${what} is not an object`);
  }
  return value as Record<string, unknown>;
}

function optionalString(object: Record<string, unknown>, key: string, what: string): string | null {
  const value = object[key];
  if (value === undefined || value === null) return null;
  if (typeof value !== "string") throw invalid(`${what}.${key} is not a string`);
  return value;
}

function requiredString(object: Record<string, unknown>, key: string, what: string): string {
  const value = object[key];
  if (typeof value !== "string") {
    throw invalid(`${what}.${key} is missing or not a string`);
  }
  if (value === "") throw invalid(`${what}.${key} is empty`);
  return value;
}

// One RemoteConfigParameterValue: `{value}` resolves to its string;
// useInAppDefault and the dynamic value objects resolve to none.
function parameterValue(value: unknown, what: string): string | null {
  const object = asObject(value, what);
  const valueField = object["value"];
  if (valueField !== undefined) {
    if (typeof valueField !== "string") throw invalid(`${what}.value is not a string`);
    return valueField;
  }
  if ("useInAppDefault" in object) {
    if (typeof object["useInAppDefault"] !== "boolean") {
      throw invalid(`${what}.useInAppDefault is not a boolean`);
    }
    return null;
  }
  if (
    "personalizationValue" in object ||
    "experimentValue" in object ||
    "rolloutValue" in object
  ) {
    return null;
  }
  // An empty object sets no value option: the parameter stays unset.
  if (Object.keys(object).length === 0) return null;
  throw invalid(`${what} sets no known value option`);
}

function parseParameter(value: unknown, what: string): ParsedParameter {
  const object = asObject(value, what);
  const rawDefault = object["defaultValue"];
  const defaultValue = rawDefault === undefined ? null : parameterValue(rawDefault, `${what}.defaultValue`);
  const conditionalValues: Record<string, string> = {};
  const rawConditional = object["conditionalValues"];
  if (rawConditional !== undefined) {
    const conditional = asObject(rawConditional, `${what}.conditionalValues`);
    for (const [name, entry] of Object.entries(conditional)) {
      const resolved = parameterValue(entry, `${what}.conditionalValues.${name}`);
      if (resolved !== null) conditionalValues[name] = resolved;
    }
  }
  return {
    defaultValue,
    conditionalValues,
    description: optionalString(object, "description", what),
    valueType: optionalString(object, "valueType", what),
  };
}

// Parses and validates the fetched document. Lenient about unknown
// fields (the REST surface is additive) and strict about the shapes it
// resolves, so a corrupt template is named, not silently half-applied.
export function parseRemoteConfigTemplate(document: unknown): ParsedRemoteConfigTemplate {
  const root = asObject(document, "the template");

  const conditions: ParsedCondition[] = [];
  const rawConditions = root["conditions"];
  if (rawConditions !== undefined) {
    if (!Array.isArray(rawConditions)) throw invalid("conditions is not an array");
    const names = new Set<string>();
    rawConditions.forEach((entry, index) => {
      const what = `conditions.${index}`;
      const object = asObject(entry, what);
      const name = requiredString(object, "name", what);
      if (names.has(name)) throw invalid(`${what}.name \`${name}\` is a duplicate`);
      names.add(name);
      conditions.push({
        name,
        expression: requiredString(object, "expression", what),
        tagColor: optionalString(object, "tagColor", what),
      });
    });
  }

  const parameters: Record<string, ParsedParameter> = {};
  const rawParameters = root["parameters"];
  if (rawParameters !== undefined) {
    for (const [key, entry] of Object.entries(asObject(rawParameters, "parameters"))) {
      parameters[key] = parseParameter(entry, `parameters.${key}`);
    }
  }

  const rawGroups = root["parameterGroups"];
  if (rawGroups !== undefined) {
    for (const [group, rawGroup] of Object.entries(asObject(rawGroups, "parameterGroups"))) {
      const what = `parameterGroups.${group}`;
      const object = asObject(rawGroup, what);
      optionalString(object, "description", what);
      const rawGroupParameters = object["parameters"];
      if (rawGroupParameters === undefined) continue;
      for (const [key, entry] of Object.entries(
        asObject(rawGroupParameters, `${what}.parameters`),
      )) {
        // The REST rule: a parameter appears once per template.
        if (key in parameters) {
          throw invalid(
            `parameter \`${key}\` appears both at the top level and in parameter group \`${group}\``,
          );
        }
        parameters[key] = parseParameter(entry, `${what}.parameters.${key}`);
      }
    }
  }

  const rawVersion = root["version"];
  const version = rawVersion === undefined ? null : asObject(rawVersion, "version");

  // The values the template resolves to for a client that evaluates no
  // conditions: with no true condition the default value stands, and
  // unset defaults are omitted.
  const resolvedValues: Record<string, string> = {};
  for (const [key, parameter] of Object.entries(parameters)) {
    if (parameter.defaultValue !== null) resolvedValues[key] = parameter.defaultValue;
  }
  return { conditions, parameters, version, resolvedValues };
}
