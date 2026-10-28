/*
 * Ejected to always render the security scheme selector, including when an
 * operation has a single scheme.
 */

import React from "react";

import { translate } from "@docusaurus/Translate";
import FormItem from "@theme/ApiExplorer/FormItem";
import FormSelect from "@theme/ApiExplorer/FormSelect";
import FormTextInput from "@theme/ApiExplorer/FormTextInput";
import { useTypedDispatch, useTypedSelector } from "@theme/ApiItem/hooks";

import {
  setAuthData,
  setSelectedAuth,
} from "docusaurus-theme-openapi-docs/lib/theme/ApiExplorer/Authorization/slice";

function Authorization() {
  const data = useTypedSelector((state: any) => state.auth.data);
  const options = useTypedSelector((state: any) => state.auth.options);
  const selected = useTypedSelector((state: any) => state.auth.selected);

  const dispatch = useTypedDispatch();

  if (selected === undefined) {
    return null;
  }

  const selectedAuth = options[selected];

  const optionKeys = Object.keys(options);

  return (
    <div>
      <FormItem>
        <FormSelect
          label={translate({
            id: "theme.openapi.auth.securityScheme",
            message: "Security Scheme",
          })}
          options={optionKeys}
          value={selected}
          onChange={(e: React.ChangeEvent<HTMLSelectElement>) => {
            dispatch(setSelectedAuth(e.target.value));
          }}
        />
      </FormItem>
      {selectedAuth.map((a: any) => {
        if (a.type === "http" && a.scheme === "bearer") {
          return (
            <FormItem key={a.key + "-bearer"}>
              <FormTextInput
                label={translate({
                  id: "theme.openapi.auth.bearerToken",
                  message: "Bearer Token",
                })}
                placeholder={translate({
                  id: "theme.openapi.auth.bearerToken",
                  message: "Bearer Token",
                })}
                password
                value={data[a.key].token ?? ""}
                onChange={(e: React.ChangeEvent<HTMLInputElement>) => {
                  const value = e.target.value;
                  dispatch(
                    setAuthData({
                      scheme: a.key,
                      key: "token",
                      value: value ? value : undefined,
                    }),
                  );
                }}
              />
            </FormItem>
          );
        }

        if (a.type === "oauth2") {
          return (
            <FormItem key={a.key + "-oauth2"}>
              <FormTextInput
                label={translate({
                  id: "theme.openapi.auth.bearerToken",
                  message: "Bearer Token",
                })}
                placeholder={translate({
                  id: "theme.openapi.auth.bearerToken",
                  message: "Bearer Token",
                })}
                password
                value={data[a.key].token ?? ""}
                onChange={(e: React.ChangeEvent<HTMLInputElement>) => {
                  const value = e.target.value;
                  dispatch(
                    setAuthData({
                      scheme: a.key,
                      key: "token",
                      value: value ? value : undefined,
                    }),
                  );
                }}
              />
            </FormItem>
          );
        }

        if (a.type === "http" && a.scheme === "basic") {
          return (
            <React.Fragment key={a.key + "-basic"}>
              <FormItem>
                <FormTextInput
                  label={translate({
                    id: "theme.openapi.auth.username",
                    message: "Username",
                  })}
                  placeholder={translate({
                    id: "theme.openapi.auth.username",
                    message: "Username",
                  })}
                  value={data[a.key].username ?? ""}
                  onChange={(e: React.ChangeEvent<HTMLInputElement>) => {
                    const value = e.target.value;
                    dispatch(
                      setAuthData({
                        scheme: a.key,
                        key: "username",
                        value: value ? value : undefined,
                      }),
                    );
                  }}
                />
              </FormItem>
              <FormItem>
                <FormTextInput
                  label={translate({
                    id: "theme.openapi.auth.password",
                    message: "Password",
                  })}
                  placeholder={translate({
                    id: "theme.openapi.auth.password",
                    message: "Password",
                  })}
                  password
                  value={data[a.key].password ?? ""}
                  onChange={(e: React.ChangeEvent<HTMLInputElement>) => {
                    const value = e.target.value;
                    dispatch(
                      setAuthData({
                        scheme: a.key,
                        key: "password",
                        value: value ? value : undefined,
                      }),
                    );
                  }}
                />
              </FormItem>
            </React.Fragment>
          );
        }

        if (a.type === "apiKey") {
          return (
            <FormItem key={a.key + "-apikey"}>
              <FormTextInput
                label={`${a.key}`}
                placeholder={`${a.key}`}
                password
                value={data[a.key].apiKey ?? ""}
                onChange={(e: React.ChangeEvent<HTMLInputElement>) => {
                  const value = e.target.value;
                  dispatch(
                    setAuthData({
                      scheme: a.key,
                      key: "apiKey",
                      value: value ? value : undefined,
                    }),
                  );
                }}
              />
            </FormItem>
          );
        }

        return null;
      })}
    </div>
  );
}

export default Authorization;
