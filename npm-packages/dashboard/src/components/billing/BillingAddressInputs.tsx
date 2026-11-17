import React from "react";
import { AddressElement } from "@stripe/react-stripe-js";
import { StripeAddressElementOptions } from "@stripe/stripe-js";
import { Address } from "generatedApi";

export function BillingAddressInputs({
  onChangeAddress,
  onChangeName,
  existingBillingAddress,
  name,
}: {
  onChangeAddress(address?: Address): Promise<void>;
  // The AddressElement always renders a name field, and its value is not part
  // of the `Address` passed to `onChangeAddress`.
  onChangeName?(name: string): Promise<void>;
  existingBillingAddress?: Address;
  name?: string;
}) {
  const addressElementOptions: StripeAddressElementOptions = {
    mode: "billing",
    defaultValues: {
      // By default, the AddressElement will only show line1 and the other fields
      // only become visible after the user starts typing their address. Setting
      // a default value for state and country will make all fields visible
      // immediately.
      address: existingBillingAddress
        ? {
            line1: existingBillingAddress.line1 || "",
            line2: existingBillingAddress.line2 || "",
            city: existingBillingAddress.city || "",
            state: existingBillingAddress.state || "",
            postal_code: existingBillingAddress.postal_code || "",
            country: existingBillingAddress.country || "",
          }
        : {
            line1: "",
            line2: "",
            city: "",
            state: "CA",
            postal_code: "",
            country: "US",
          },
      // Prefill the name field from the billing contact on the subscription if
      // we can. It's a little annoying that we ask them to enter in their name
      // in two places, but I don't see a way in StripeAddressElementOptions to
      // hide the name field.
      name,
    },
  };

  return (
    <div className="flex flex-col gap-2">
      <AddressElement
        className="w-full"
        options={addressElementOptions}
        onChange={(event) => {
          void onChangeName?.(event.value.name);
          // Call onChangeAddress with the full billing address when all
          // required fields are filled or otherwise undefined to clear the
          // address (in case the user deletes some previously filled fields).
          if (event.complete) {
            void onChangeAddress(event.value.address);
          } else {
            void onChangeAddress(undefined);
          }
        }}
      />
    </div>
  );
}
