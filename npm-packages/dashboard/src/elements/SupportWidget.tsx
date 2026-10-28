import {
  ChatBubbleIcon,
  ChevronLeftIcon,
  DiscordLogoIcon,
  ExternalLinkIcon,
} from "@radix-ui/react-icons";
import { captureException, captureMessage } from "@sentry/nextjs";
import { Button } from "@ui/Button";
import { ClosePanelButton } from "@ui/ClosePanelButton";
import { toast } from "@common/lib/utils";
import { Sheet } from "@ui/Sheet";
import { TextInput } from "@ui/TextInput";
import { useFormik } from "formik";
import { useCurrentDeployment } from "api/deployments";
import { useCurrentTeam } from "api/teams";
import { useCurrentProject } from "api/projects";
import { useTeamOrbSubscription } from "api/billing";
import { useProfile } from "api/profile";
import { useAuthHeader } from "hooks/fetching";
import { createGlobalState } from "react-use";
import * as Yup from "yup";
import { useWorkOS } from "hooks/useWorkOS";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";

export const useSupportFormOpen = createGlobalState<
  { defaultMessage: string; defaultSubject: string } | boolean
>(false);

export function SupportWidget() {
  const team = useCurrentTeam();
  const { subscription } = useTeamOrbSubscription(team?.id);
  const { user } = useWorkOS();
  const { supportTicketForAllPlans } = useLaunchDarkly();
  const [openState, setOpenState] = useSupportFormOpen();

  const canSubmitTicket =
    supportTicketForAllPlans ||
    (subscription &&
      (subscription.plan.planType === "CONVEX_PROFESSIONAL" ||
        subscription.plan.planType === "CONVEX_BUSINESS"));
  if (openState === false || !user) {
    return null;
  }

  return (
    <Sheet
      className="absolute bottom-0 z-50 w-screen animate-fadeInFromLoading p-4 shadow-2xl transition-all sm:right-8 sm:bottom-8 sm:w-[24rem]"
      padding={false}
    >
      {openState === true ? (
        <>
          <div className="mb-2 flex justify-between">
            <h5>Get in touch</h5>
            <ClosePanelButton
              onClose={() => setOpenState(false)}
              className="ml-auto"
            />
          </div>
          <p className="mb-4 text-xs text-content-secondary">
            Discord is a great way to get a quick response from the Convex
            community or to ask an AI for help!
          </p>
          <div className="flex flex-col">
            <Button
              href="https://docs.convex.dev"
              className="text-content-primary"
              icon={<ExternalLinkIcon />}
              inline
              target="_blank"
            >
              Convex Documentation
            </Button>
            <Button
              inline
              href="https://convex.dev/community"
              className="text-content-primary"
              icon={<DiscordLogoIcon />}
              target="_blank"
            >
              Join the Discord community
            </Button>
            <Button
              inline
              onClick={() =>
                setOpenState({ defaultSubject: "", defaultMessage: "" })
              }
              icon={<ChatBubbleIcon />}
              tip={
                !canSubmitTicket &&
                "Email support is available on the Pro plan."
              }
              tipSide="left"
              disabled={!canSubmitTicket}
              className={canSubmitTicket ? "text-content-primary" : ""}
            >
              File a support ticket{" "}
              {!canSubmitTicket && (
                <span
                  className="w-fit rounded-sm bg-util-accent px-1.5 py-0.5 text-xs font-semibold tracking-wider text-white uppercase"
                  title="Only available on the Pro plan"
                >
                  Pro
                </span>
              )}
            </Button>
          </div>
        </>
      ) : (
        <SupportForm />
      )}
    </Sheet>
  );
}

function SupportForm() {
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const deployment = useCurrentDeployment();

  const profile = useProfile();

  const [openState, setOpenState] = useSupportFormOpen();
  const authHeader = useAuthHeader();

  const formState = useFormik({
    initialValues: {
      subject:
        typeof openState === "object" ? openState.defaultSubject || "" : "",
      message:
        typeof openState === "object" ? openState.defaultMessage || "" : "",
    },
    validationSchema: Yup.object({
      subject: Yup.string().max(128).required(),
      message: Yup.string().max(2500).required(),
    }),
    enableReinitialize: true,
    isInitialValid:
      typeof openState === "object" &&
      !!openState.defaultSubject &&
      !!openState.defaultMessage,
    onSubmit: async (values) => {
      if (!team) {
        // Team hasn't loaded in yet. Shouldn't ever happen.
        toast("error", "Failed to send message, please try again.");
        return;
      }

      const resp = await fetch("/api/contact-form", {
        method: "POST",
        body: JSON.stringify({
          ...values,
          teamId: team?.id,
          projectId: project?.id,
          deploymentName: deployment?.name,
        }),
        headers: {
          "Content-Type": "application/json",
          Authorization: authHeader,
        },
      });

      if (!resp.ok) {
        try {
          if (resp.status < 500 || resp.status >= 400) {
            const { error } = await resp.json();
            captureMessage(error, "error");
          }
        } catch (e) {
          captureException(e);
        }

        toast(
          "error",
          "Failed to send message. Please try again or email us at support@convex.dev",
          undefined,
          false,
        );
        return;
      }

      setOpenState(false);
      toast("success", "Message sent!");
    },
  });

  return (
    <form className="flex flex-col gap-4" onSubmit={formState.handleSubmit}>
      <div className="flex items-center justify-between">
        <Button
          inline
          size="xs"
          variant="neutral"
          icon={<ChevronLeftIcon />}
          onClick={() => setOpenState(true)}
        />
        <h5>Get in touch</h5>
        <ClosePanelButton
          onClose={() => setOpenState(false)}
          className="ml-auto"
        />
      </div>
      <TextInput
        label="Subject"
        id="subject"
        required
        autoFocus
        onChange={formState.handleChange}
        value={formState.values.subject}
        error={formState.errors.subject}
      />
      <label
        htmlFor="message"
        className="flex flex-col gap-1 text-sm text-content-primary"
      >
        Message
        <textarea
          id="message"
          name="message"
          className="h-48 resize-y rounded-sm border bg-background-secondary px-4 py-2 text-content-primary placeholder:text-content-tertiary focus:border-border-selected focus:outline-hidden"
          required
          onChange={formState.handleChange}
          value={formState.values.message}
          placeholder="How can we help you?"
        />
        {formState.errors.message && (
          <p
            className="flex max-w-prose gap-1 text-xs text-content-errorSecondary"
            role="alert"
          >
            {formState.errors.message}
          </p>
        )}
      </label>
      <Button
        type="submit"
        className="justify-center"
        disabled={!formState.isValid}
        loading={formState.isSubmitting}
      >
        Send
      </Button>
      {profile?.email && (
        <p className="text-xs text-content-secondary">
          The Convex support team will respond by email to {profile.email}.
        </p>
      )}
    </form>
  );
}
