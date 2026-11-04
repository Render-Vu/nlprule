;;; nlprule-complete-tests.el --- Tests for nlprule-complete  -*- lexical-binding: t; -*-

;; Run with:
;;   emacs -Q --batch -L emacs -l emacs/nlprule-complete-tests.el -f ert-run-tests-batch-and-exit
;; The integration tests run only if nlprule-server answers at
;; $NLPRULE_SERVER_URL (default http://localhost:8011) with the Spanish binaries.

;;; Code:

(require 'ert)
(require 'cl-lib)
(require 'nlprule-complete)

(defvar nlprule-complete-tests--url
  (or (getenv "NLPRULE_SERVER_URL") "http://localhost:8011"))

(defun nlprule-complete-tests--server-up-p ()
  (let ((nlprule-complete-url nlprule-complete-tests--url))
    (ignore-errors
      (let* ((url-show-status nil)
             (buffer (url-retrieve-synchronously
                      (concat nlprule-complete-url "/health") t t 2)))
        (when buffer (kill-buffer buffer) t)))))

(defmacro nlprule-complete-tests--with-text (text &rest body)
  "Run BODY in a text buffer containing TEXT with point at the end."
  (declare (indent 1))
  `(with-temp-buffer
     (text-mode)
     (insert ,text)
     (let ((nlprule-complete-url nlprule-complete-tests--url)
           (nlprule-complete--offline-until 0))
       ,@body)))

(defun nlprule-complete-tests--complete ()
  "Call the capf; return (BEG END CANDIDATES-IN-DISPLAY-ORDER)."
  (let* ((result (nlprule-complete-at-point))
         (beg (nth 0 result))
         (end (nth 1 result))
         (table (nth 2 result))
         (prefix (buffer-substring-no-properties beg end))
         (metadata (completion-metadata prefix table nil))
         (sort (completion-metadata-get metadata 'display-sort-function))
         (all (all-completions prefix table)))
    (list beg end (funcall sort all))))

;;; Unit tests (no server)

(ert-deftest nlprule-complete-encodes-form ()
  (should (equal (nlprule-complete--encode '(("text" . "Los niños jue") ("limit" . "3")))
                 "text=Los%20ni%C3%B1os%20jue&limit=3")))

(ert-deftest nlprule-complete-table-keeps-order ()
  (let* ((nlprule-complete--offline-until (+ (float-time) 60)) ; no requests
         (table (nlprule-complete--table "Los niños " "jue"
                                         '("juegan" "jueguen" "juerguean")))
         (metadata (completion-metadata "jue" table nil)))
    (should (eq (completion-metadata-get metadata 'display-sort-function) #'identity))
    (should (equal (all-completions "jue" table) '("juegan" "jueguen" "juerguean")))
    ;; offline: a longer prefix filters the known candidates
    (should (equal (all-completions "jueg" table) '("juegan" "jueguen")))))

(ert-deftest nlprule-complete-table-refetches ()
  (let* ((requests nil)
         (table (nlprule-complete--table "Los niños " "jue" '("juegan"))))
    (cl-letf (((symbol-function 'nlprule-complete--request)
               (lambda (text)
                 (push text requests)
                 '((prefix . "juer") (items ((word . "juerguean")))))))
      (should (equal (all-completions "juer" table) '("juerguean")))
      (should (equal (all-completions "juer" table) '("juerguean")))
      ;; one request, with the original context, cached afterwards
      (should (equal requests '("Los niños juer")))
      ;; a too short prefix asks nothing
      (should (equal (all-completions "j" table) '("juegan")))
      (should (= (length requests) 1)))))

(ert-deftest nlprule-complete-annotates ()
  (let ((candidate (nlprule-complete--propertize
                    '((word . "juegan") (tags "VMIP3P0") (lemmas "jugar")
                      (matches_context . t)))))
    (should (equal (nlprule-complete--annotate candidate) "  VMIP3P0 · jugar"))
    (let ((nlprule-complete-annotate nil))
      (should-not (nlprule-complete--annotate candidate)))))

(ert-deftest nlprule-complete-only-matching ()
  (let ((answer '((items ((word . "juegan") (matches_context . t))
                         ((word . "juez") (matches_context . nil))))))
    (should (equal (nlprule-complete--candidates answer) '("juegan" "juez")))
    (let ((nlprule-complete-only-matching t))
      (should (equal (nlprule-complete--candidates answer) '("juegan"))))))

(ert-deftest nlprule-complete-short-prefix-or-mid-word-does-nothing ()
  (nlprule-complete-tests--with-text "Los niños j"
    (should-not (nlprule-complete-at-point)))
  (nlprule-complete-tests--with-text "Los niños juegan"
    (goto-char (- (point-max) 2))
    (should-not (nlprule-complete-at-point))))

(ert-deftest nlprule-complete-offline-is-silent ()
  (nlprule-complete-tests--with-text "Los niños jue"
    ;; nothing listens on port 9
    (let ((nlprule-complete-url "http://127.0.0.1:9"))
      (should-not (nlprule-complete-at-point))
      (should (> nlprule-complete--offline-until (float-time)))
      (should nlprule-complete--last-error))))

(ert-deftest nlprule-complete-mode-adds-capf ()
  (with-temp-buffer
    (nlprule-complete-mode 1)
    (should (memq #'nlprule-complete-at-point completion-at-point-functions))
    (nlprule-complete-mode -1)
    (should-not (memq #'nlprule-complete-at-point completion-at-point-functions))))

;;; Integration tests (need the server)

(ert-deftest nlprule-complete-server-agreement ()
  (skip-unless (nlprule-complete-tests--server-up-p))
  (nlprule-complete-tests--with-text "Hola.\n\nLos niños jue"
    (pcase-let ((`(,beg ,end ,candidates) (nlprule-complete-tests--complete)))
      (should (equal (buffer-substring beg end) "jue"))
      (should (= end (point-max)))
      (should (equal (car candidates) "juegan"))
      (should (equal (nlprule-complete--annotate (car candidates))
                     "  VMIP3P0 · jugar")))))

(ert-deftest nlprule-complete-server-subjunctive-and-case ()
  (skip-unless (nlprule-complete-tests--server-up-p))
  (nlprule-complete-tests--with-text "Si nosotros Escrib"
    (pcase-let ((`(,_ ,_ ,candidates) (nlprule-complete-tests--complete)))
      (should (member (car candidates) '("Escribiéramos" "Escribiésemos"))))))

(ert-deftest nlprule-complete-server-long-buffer-offsets ()
  (skip-unless (nlprule-complete-tests--server-up-p))
  ;; context is cut at `nlprule-complete-context-chars'; offsets must still map back
  (nlprule-complete-tests--with-text
      (concat (make-string 1000 ?a) " ñandú. Las casas bonit")
    (pcase-let ((`(,beg ,end ,candidates) (nlprule-complete-tests--complete)))
      (should (equal (buffer-substring beg end) "bonit"))
      (should (equal (car candidates) "bonitas")))))

(ert-deftest nlprule-complete-completion-at-point-inserts ()
  (skip-unless (nlprule-complete-tests--server-up-p))
  (nlprule-complete-tests--with-text "Las niñas son bonit"
    (setq-local completion-at-point-functions '(nlprule-complete-at-point))
    (setq-local completion-styles '(basic))
    ;; several candidates: completes the common part; insert the first one
    (let* ((result (nlprule-complete-at-point)))
      (completion--replace (nth 0 result) (nth 1 result) "bonitas")
      (should (equal (buffer-string) "Las niñas son bonitas")))))

;;; corfu (if installed, e.g. with -L path/to/corfu -L path/to/compat)

(ert-deftest nlprule-complete-corfu-candidates ()
  (skip-unless (and (require 'corfu nil t) (nlprule-complete-tests--server-up-p)))
  (nlprule-complete-tests--with-text "Los niños jue"
    (pcase (corfu--capf-wrapper #'nlprule-complete-at-point 2)
      (`(,_ ,beg ,end ,_ :corfu--state ,state . ,_)
       (should (equal (buffer-substring beg end) "jue"))
       ;; corfu keeps the server order
       (should (equal (car (alist-get 'corfu--candidates state)) "juegan")))
      (other (ert-fail (list "no corfu state" other))))
    ;; typing on: corfu recomputes with the same table, which asks the server again
    (pcase-let ((`(,beg ,end ,table . ,_) (nlprule-complete-at-point)))
      (insert "r")
      (let ((state (corfu--compute (cons (buffer-substring beg (point)) (- (point) beg))
                                   table nil)))
        (ignore end)
        (should (equal (car (alist-get 'corfu--candidates state)) "juerguean"))))))

(provide 'nlprule-complete-tests)
;;; nlprule-complete-tests.el ends here
