import { captureFeedback, captureMessage } from "@sentry/nextjs";
import { Button } from "@ui/Button";
import { ClosePanelButton } from "@ui/ClosePanelButton";
import { Sheet } from "@ui/Sheet";
import { TextInput } from "@ui/TextInput";
import { toast } from "@common/lib/utils";
import { FeedbackContext } from "@common/lib/deploymentContext";
import { useFormik } from "formik";
import { createGlobalState } from "react-use";
import * as Yup from "yup";
import { useProfile } from "api/profile";

// Set to the context of whatever asked for feedback, and null when closed.
// Mounted outside the components that open it (the command palette unmounts on
// close), so it outlives whatever triggered it.
export const useFeedbackFormOpen = createGlobalState<FeedbackContext | null>(
  null,
);

export const commandPaletteFeedback: FeedbackContext = {
  event: "Command palette feedback",
  title: "Send feedback",
  prompt: "Tell us what you were looking for in the command palette.",
  question: "What were you looking for?",
};

// Replaces Sentry's injected `showReportDialog` with a form styled like the
// support form: the name/email are shown but fixed to the signed-in profile,
// and only the message is editable.
export function FeedbackForm() {
  const [context, setContext] = useFeedbackFormOpen();
  const profile = useProfile();

  const formState = useFormik({
    initialValues: { message: "" },
    validationSchema: Yup.object({
      message: Yup.string().max(2500).required(),
    }),
    isInitialValid: false,
    onSubmit: (values) => {
      const eventId = captureMessage(
        context?.event ?? "Dashboard feedback",
        "info",
      );
      void captureFeedback({
        associatedEventId: eventId,
        name: profile?.name ?? "Unknown",
        email: profile?.email ?? "unknown@convex.dev",
        message: values.message,
      });
      setContext(null);
      formState.resetForm();
      toast("success", "Thanks for the feedback!");
    },
  });

  if (!context) {
    return null;
  }

  return (
    <Sheet
      className="absolute bottom-0 z-50 w-screen animate-fadeInFromLoading p-4 shadow-2xl transition-all sm:right-8 sm:bottom-8 sm:w-[24rem]"
      padding={false}
    >
      <form className="flex flex-col gap-4" onSubmit={formState.handleSubmit}>
        <div className="flex items-center justify-between">
          <h5>{context.title}</h5>
          <ClosePanelButton
            onClose={() => setContext(null)}
            className="ml-auto"
          />
        </div>
        <p className="text-xs text-content-secondary">{context.prompt}</p>
        {/* Name and email are fixed to the signed-in profile. */}
        <TextInput
          label="Name"
          id="feedback-name"
          value={profile?.name ?? ""}
          disabled
          readOnly
        />
        <TextInput
          label="Email"
          id="feedback-email"
          type="email"
          value={profile?.email ?? ""}
          disabled
          readOnly
        />
        <label
          htmlFor="message"
          className="flex flex-col gap-1 text-sm text-content-primary"
        >
          {context.question}
          <textarea
            id="message"
            name="message"
            className="h-32 resize-y rounded-sm border bg-background-secondary px-4 py-2 text-content-primary placeholder:text-content-tertiary focus:border-border-selected focus:outline-hidden"
            required
            autoFocus
            onChange={formState.handleChange}
            value={formState.values.message}
            placeholder={context.question}
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
      </form>
    </Sheet>
  );
}
