import { Button } from "@ui/Button";
import { Sheet } from "@ui/Sheet";
import { LoadingLogo } from "@ui/Loading";
import { useAcceptOptIns, useHasOptedIn } from "api/optins";
import { useRouter } from "next/router";
import { ChangeEvent, useEffect, useState } from "react";
import { Link } from "@ui/Link";

// TODO get these from the server once there are more of them
const OPT_IN_MESSAGES: Record<
  string,
  { text: string; linkText: string; linkUrl: string }
> = {
  tos: {
    text: "I’ve read and accept the",
    linkText: "Terms of Service",
    linkUrl: "https://www.convex.dev/legal/tos",
  },
};
type OptInName = keyof typeof OPT_IN_MESSAGES;

export function CheckboxLine({
  optInName,
  toggle,
}: {
  optInName: OptInName;
  toggle: (optInName: OptInName, checked: boolean) => void;
}) {
  const { text, linkUrl, linkText } = OPT_IN_MESSAGES[optInName];
  const [checked, setChecked] = useState(false);
  const onChange = (e: ChangeEvent<HTMLInputElement>) => {
    setChecked(e.target.checked);
    toggle(optInName, e.target.checked);
  };

  return (
    <div className="flex items-center">
      <input
        id={optInName}
        type="checkbox"
        checked={checked}
        onChange={onChange}
        className="mr-2 cursor-pointer accent-util-accent"
        style={{
          fontSize: 40,
        }}
      />{" "}
      <label
        htmlFor={optInName}
        className="cursor-pointer text-sm text-content-primary"
      >
        <span>
          {text}{" "}
          <Link href={linkUrl} target="_blank" rel="noreferrer">
            {linkText}
          </Link>
          .
        </span>
      </label>
    </div>
  );
}

export function LoginTerms() {
  const router = useRouter();
  const { optInsWithMessageToAccept, hasOptedIn } = useHasOptedIn();
  const acceptOptIns = useAcceptOptIns();
  const [newOptIns, setNewOptIns] = useState<Set<OptInName>>(new Set());

  const toggle = (optInName: OptInName, value: boolean) => {
    setNewOptIns((prevNewOptIns) => {
      const s = new Set(prevNewOptIns);
      s.delete(optInName);
      if (value) s.add(optInName);
      return s;
    });
  };

  useEffect(() => {
    if (hasOptedIn) {
      const current = new URL(window.location.href);
      const pathname = (router.query.from as string) || "/";
      // Require that the URL we redirect to is same-origin.
      let from = new URL(pathname, `${current.protocol}//${current.host}`);
      if (current.origin !== from.origin) {
        from = new URL(current.toString());
        from.pathname = "/";
      }
      void router.push(from);
    }
  }, [hasOptedIn, router]);

  const [isAccepting, setIsAccepting] = useState(false);

  if (!optInsWithMessageToAccept) {
    return <LoadingLogo />;
  }

  const needsAcceptNames = optInsWithMessageToAccept.map(
    (optIn) => Object.keys(optIn.optIn)[0],
  );
  const acceptedAll =
    needsAcceptNames.filter((name) => !newOptIns.has(name)).length === 0;

  const onContinue = acceptedAll
    ? async () => {
        setIsAccepting(true);
        try {
          await acceptOptIns(optInsWithMessageToAccept.map((x) => x.optIn));
        } catch (e) {
          setIsAccepting(false);
          throw e;
        }
      }
    : undefined;

  if (isAccepting || optInsWithMessageToAccept.length === 0) {
    return <LoadingLogo />;
  }

  return (
    <div className="flex flex-col items-center">
      <div className="mb-4 text-sm text-content-primary">
        Welcome to Convex! We need you to take a look at these before we
        continue.
      </div>
      <Sheet className="w-fit">
        {optInsWithMessageToAccept.map((optIn) => {
          const optInName = Object.keys(optIn.optIn)[0];
          if (!(optInName in OPT_IN_MESSAGES)) {
            throw new Error(
              `No UI code to display opt in ${optInName} ${JSON.stringify(
                optIn,
              )}`,
            );
          }
          return (
            <CheckboxLine
              key={optInName}
              optInName={optInName}
              toggle={toggle}
            />
          );
        })}
      </Sheet>
      <div className="flex justify-center pt-4">
        <Button onClick={onContinue} disabled={!onContinue}>
          Continue
        </Button>
      </div>
    </div>
  );
}
