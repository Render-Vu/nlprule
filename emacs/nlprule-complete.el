;;; nlprule-complete.el --- Grammar-aware completion from nlprule-server  -*- lexical-binding: t; -*-

;; Version: 0.1.0
;; Package-Requires: ((emacs "27.1"))
;; Keywords: convenience, completion, languages
;; URL: https://github.com/render-vu/nlprule

;;; Commentary:

;; A `completion-at-point' function backed by the `/complete' endpoint of
;; nlprule-server (see server/README.md).  It works with any capf frontend;
;; with corfu, enable `nlprule-complete-mode' in text buffers:
;;
;;   (use-package nlprule-complete
;;     :load-path "~/src/nlprule/emacs"
;;     :hook ((text-mode org-mode markdown-mode) . nlprule-complete-mode))
;;
;; The server infers the expected part of speech from the words before point
;; ("Los niños jue" -> "juegan") and returns the candidates already ranked;
;; this package keeps that order (it disables the frontend's sorting for its
;; candidates) and shows the tag and lemma as annotation.
;;
;; Requests are synchronous with a short timeout.  If the server is not
;; reachable, the function stays silent for `nlprule-complete-retry-delay'
;; seconds and other capfs take over.

;;; Code:

(require 'url)
(require 'url-http)
(require 'json)
(require 'subr-x)
(require 'seq)

(defvar url-http-end-of-headers)
(defvar url-http-attempt-keepalives)

(defgroup nlprule-complete nil
  "Grammar-aware completion from nlprule-server."
  :group 'convenience
  :prefix "nlprule-complete-")

(defcustom nlprule-complete-url "http://localhost:8011"
  "Base URL of nlprule-server."
  :type 'string)

(defcustom nlprule-complete-limit 10
  "Maximum number of candidates requested."
  :type 'integer)

(defcustom nlprule-complete-min-prefix 2
  "Minimum number of characters typed before asking the server."
  :type 'integer)

(defcustom nlprule-complete-timeout 0.5
  "Seconds to wait for the server before giving up on a request."
  :type 'number)

(defcustom nlprule-complete-retry-delay 30
  "Seconds to stay silent after the server could not be reached."
  :type 'number)

(defcustom nlprule-complete-context-chars 300
  "Characters of text before point sent as context.
The context never crosses the start of the paragraph."
  :type 'integer)

(defcustom nlprule-complete-validate nil
  "If non-nil, ask the server to drop candidates its rules flag as errors.
Slower: every candidate is checked with the grammar rules."
  :type 'boolean)

(defcustom nlprule-complete-only-matching nil
  "If non-nil, only show candidates that fit the inferred context.
When nothing fits, all candidates are shown."
  :type 'boolean)

(defcustom nlprule-complete-annotate t
  "If non-nil, annotate candidates with their tags and lemmas."
  :type 'boolean)

(defvar nlprule-complete--offline-until 0
  "Time (as a float) until which the server is considered unreachable.")

(defvar nlprule-complete--last-error nil
  "The last error talking to the server, for `nlprule-complete-status'.")

(defun nlprule-complete--encode (params)
  "Encode PARAMS, an alist of (KEY . VALUE) strings, as a form body."
  (mapconcat (lambda (pair)
               (concat (url-hexify-string (car pair)) "="
                       (url-hexify-string (cdr pair))))
             params "&"))

(defun nlprule-complete--fetch (url timeout)
  "Retrieve URL and return its buffer, or nil after TIMEOUT seconds.
Unlike `url-retrieve-synchronously', which polls every 50 ms, this
waits in 5 ms steps: the server answers in a few milliseconds.
Keep-alive is off: on a reused connection the server's small writes
meet delayed ACKs and every answer takes ~40 ms more."
  (let* ((done nil)
         (url-http-attempt-keepalives nil)
         (deadline (+ (float-time) timeout))
         (buffer (url-retrieve url (lambda (&rest _) (setq done t)) nil t t)))
    (while (and (not done) (< (float-time) deadline))
      (accept-process-output nil 0.005))
    (if done
        buffer
      (when-let ((process (get-buffer-process buffer)))
        (delete-process process))
      (kill-buffer buffer)
      nil)))

(defun nlprule-complete--post (path params)
  "POST PARAMS to PATH on the server and return the parsed JSON, or nil."
  (let* ((url-request-method "POST")
         (url-request-extra-headers
          '(("Content-Type" . "application/x-www-form-urlencoded; charset=utf-8")))
         (url-request-data (encode-coding-string
                            (nlprule-complete--encode params) 'utf-8))
         (url-show-status nil)
         (buffer (nlprule-complete--fetch
                  (concat (string-remove-suffix "/" nlprule-complete-url) path)
                  nlprule-complete-timeout)))
    (unless buffer
      (error "No answer from %s within %ss" nlprule-complete-url
             nlprule-complete-timeout))
    (unwind-protect
        (with-current-buffer buffer
          (goto-char (point-min))
          (let ((status (and (looking-at "HTTP/[0-9.]+ \\([0-9]+\\)")
                             (string-to-number (match-string 1)))))
            (unless url-http-end-of-headers
              (error "Malformed answer from %s" nlprule-complete-url))
            (goto-char url-http-end-of-headers)
            (let* ((body (decode-coding-string
                          (buffer-substring-no-properties (point) (point-max))
                          'utf-8))
                   (json (json-parse-string body :object-type 'alist
                                            :array-type 'list
                                            :false-object nil :null-object nil)))
              (unless (eql status 200)
                (error "Server error %s: %s" status (alist-get 'error json)))
              json)))
      (kill-buffer buffer))))

(defun nlprule-complete--context-start ()
  "Start of the context sent: limited by chars and by the paragraph start."
  (max (- (point) nlprule-complete-context-chars)
       (save-excursion (backward-paragraph) (point))
       (point-min)))

(defun nlprule-complete--request (text)
  "Ask the server to complete the end of TEXT.
Return the parsed answer, or nil if the server is offline."
  (when (> (float-time) nlprule-complete--offline-until)
    (condition-case err
        (nlprule-complete--post
         "/complete"
         `(("text" . ,text)
           ("limit" . ,(number-to-string nlprule-complete-limit))
           ("validate" . ,(if nlprule-complete-validate "true" "false"))))
      (error
       (setq nlprule-complete--last-error (error-message-string err)
             nlprule-complete--offline-until
             (+ (float-time) nlprule-complete-retry-delay))
       nil))))

(defun nlprule-complete--propertize (item)
  "Turn a completion ITEM of the server answer into a candidate string."
  (let ((word (alist-get 'word item)))
    (propertize word
                'nlprule-tags (alist-get 'tags item)
                'nlprule-lemmas (alist-get 'lemmas item)
                'nlprule-matches (alist-get 'matches_context item))))

(defun nlprule-complete--candidates (answer)
  "Candidate strings from the server ANSWER, best first."
  (let* ((items (alist-get 'items answer))
         (matching (seq-filter (lambda (i) (alist-get 'matches_context i)) items)))
    (mapcar #'nlprule-complete--propertize
            (if (and nlprule-complete-only-matching matching) matching items))))

(defun nlprule-complete--annotate (candidate)
  "Annotation for CANDIDATE: its first tags and its lemmas."
  (when nlprule-complete-annotate
    (let ((tags (get-text-property 0 'nlprule-tags candidate))
          (lemmas (get-text-property 0 'nlprule-lemmas candidate)))
      (when (or tags lemmas)
        (concat "  "
                (string-join (seq-take tags 2) ",")
                (when lemmas (concat " · " (string-join lemmas ", "))))))))

(defun nlprule-complete--table (context prefix candidates)
  "A completion table that keeps the server order of its candidates.
CANDIDATES are the server's answer for PREFIX typed after CONTEXT.
When the frontend asks for another prefix (e.g. corfu while typing
continues), the server is asked again with the same CONTEXT; answers
are cached per prefix."
  (let ((cache (make-hash-table :test #'equal)))
    (puthash prefix candidates cache)
    (lambda (string pred action)
      (if (eq action 'metadata)
          '(metadata (category . nlprule)
                     (display-sort-function . identity)
                     (cycle-sort-function . identity))
        (let ((found (gethash string cache 'missing)))
          (when (eq found 'missing)
            (setq found
                  (if (< (length string) nlprule-complete-min-prefix)
                      nil
                    (let ((answer (nlprule-complete--request
                                   (concat context string))))
                      ;; only use the answer if the server completed STRING
                      (and answer
                           (equal (alist-get 'prefix answer) string)
                           (nlprule-complete--candidates answer)))))
            ;; fall back to filtering what is known for a shorter prefix
            (unless found
              (setq found (all-completions string (gethash prefix cache))))
            (puthash string found cache))
          (complete-with-action action found string pred))))))

;;;###autoload
(defun nlprule-complete-at-point ()
  "Complete the word before point with nlprule-server.
Meant for `completion-at-point-functions'."
  (let* ((context-start (nlprule-complete--context-start))
         (prefix-start (save-excursion
                         (skip-syntax-backward "w" context-start)
                         (point))))
    (when (and (>= (- (point) prefix-start) nlprule-complete-min-prefix)
               ;; only at the end of a word
               (not (looking-at-p "\\w")))
      (when-let* ((text (buffer-substring-no-properties context-start (point)))
                  (answer (nlprule-complete--request text))
                  (candidates (nlprule-complete--candidates answer)))
        ;; the server reports where the prefix starts, in chars of the sent text
        (let* ((start (alist-get 'start answer))
               (beg (+ context-start start))
               (end (+ context-start (alist-get 'end answer))))
          (list beg end
                (nlprule-complete--table (substring text 0 start)
                                         (alist-get 'prefix answer)
                                         candidates)
                :exclusive 'no
                :annotation-function #'nlprule-complete--annotate
                :company-kind (lambda (_) 'text)))))))

;;;###autoload
(defun nlprule-complete-status ()
  "Check whether nlprule-server answers, and reset the offline state."
  (interactive)
  (setq nlprule-complete--offline-until 0)
  (condition-case err
      (let* ((url-show-status nil)
             (buffer (nlprule-complete--fetch
                      (concat (string-remove-suffix "/" nlprule-complete-url)
                              "/health")
                      2)))
        (if buffer
            (progn (kill-buffer buffer)
                   (message "nlprule-server answers at %s" nlprule-complete-url))
          (message "nlprule-server does not answer at %s" nlprule-complete-url)))
    (error (message "nlprule-server: %s (last error: %s)"
                    (error-message-string err) nlprule-complete--last-error))))

;;;###autoload
(define-minor-mode nlprule-complete-mode
  "Grammar-aware completion at point from nlprule-server."
  :lighter " nlpc"
  (if nlprule-complete-mode
      (add-hook 'completion-at-point-functions #'nlprule-complete-at-point -10 t)
    (remove-hook 'completion-at-point-functions #'nlprule-complete-at-point t)))

(provide 'nlprule-complete)
;;; nlprule-complete.el ends here
