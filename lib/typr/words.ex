defmodule Typr.Words do
  @moduledoc """
  Word lists and test-text generation.

  Two vocabularies ship with `typr`: `english`, the most frequent words in the
  language, and `english_extended`, which adds longer and less common words.
  Both can be decorated with punctuation and numbers the way monkeytype does.
  """

  @english ~w(
    the be to of and a in that have it for not on with he as you do at this but
    his by from they we say her she or an will my one all would there their what
    so up out if about who get which go me when make can like time no just him
    know take people into year your good some could them see other than then now
    look only come its over think also back after use two how our work first well
    way even new want because any these give day most us man find here thing tell
    very still should much need right old too same life world own feel three high
    place small large next early young important few public bad able part hand eye
    woman child home water room mother area money story fact month night book word
    side kind head house friend father hour game line end member car city name team
    minute idea kid body face level door health person art war history party result
    change morning reason research girl guy moment air teacher force education foot
    boy age policy process music market sense nation plan college interest death
    course someone experience behind reach local sure president road table sport
    talk turn start might hard open walk white number group show run move live
    believe hold bring happen write sit stand lose pay meet include continue set
    learn lead understand watch follow stop create speak read spend grow drive
    break call try ask keep let begin seem help
  )

  @english_extended ~w(
    absolute achieve alongside amount analysis appear approach argument assume
    balance benefit beyond capital carefully category challenge character
    circumstance collection combine comfortable community complete concern
    condition confidence connection consider constant contain context
    contribute conversation critical culture current decision definitely
    describe design detail determine develop difference direction discover
    discussion distance distribute economy effective effort element encourage
    energy environment equipment establish evidence exactly example excellent
    exchange exercise expect explain express extremely feature figure finally
    financial function further generation generous global hardly however
    identify imagine immediate improve include increase indicate individual
    industry influence information initial instead institution intelligence
    introduce investment involve knowledge language largely leadership
    literature machine maintain majority management material measure mention
    method modern movement natural necessary negative neighborhood network
    normally obviously occasion opportunity organize original otherwise
    particular perform perhaps performance permanent personal perspective
    physical popular position possible potential practice prepare presence
    prevent previous private probably produce professional program property
    protect provide purpose quality quarter question quickly realize
    recognize recommend reduce reflect regular relationship relative remain
    remember remove replace represent require resource respond responsible
    restaurant returning revenue satisfy schedule science section security
    separate serious service several signal significant similar situation
    society software solution somewhere specific standard strategy structure
    successful sufficient suggest support suppose surface surround survive
    technology television temperature terrible therefore thousand throughout
    tomorrow tradition traffic transfer treatment tremendous typical
    understand university unusual valuable variety vehicle version violence
    vision volume welcome whatever whenever wherever whether whisper without
    wonderful writing yesterday
  )

  # Pangrams and plain proverbs, written for this project so the text stays
  # unencumbered. Quote mode draws from these.
  @sentences [
    "the quick brown fox jumps over the lazy dog while the sleepy cat watches from a sunny window",
    "pack my box with five dozen liquor jugs and ship it out before the harbor freezes over",
    "how vexingly quick daft zebras jump when a jackal wanders past the waterhole at dusk",
    "a journey of a thousand miles begins with a single step taken in the right direction",
    "measure twice and cut once because the board never grows back once you have trimmed it",
    "the best time to plant a tree was twenty years ago and the second best time is today",
    "practice does not make perfect but it does make permanent so practice the thing you want to keep",
    "a smooth sea never made a skilled sailor and calm weather teaches nothing about the storm",
    "speed comes from accuracy repeated often enough that your hands stop asking your eyes for help",
    "the person who moves a mountain begins by carrying away one small stone at a time",
    "we judge ourselves by what we mean to do and everyone else by what they actually did",
    "sharpen the axe before you swing it or you will spend the whole day making noise in the forest"
  ]

  @punctuation_marks [".", ".", ".", ",", ",", ";", ":", "!", "?"]

  @doc "Names of the available word lists."
  @spec list_names() :: [String.t()]
  def list_names, do: ["english", "english_extended"]

  @doc "The raw vocabulary behind a list name."
  @spec vocabulary(String.t()) :: [String.t()] | nil
  def vocabulary("english"), do: @english
  def vocabulary("english_extended"), do: @english_extended
  def vocabulary(_), do: nil

  @doc "A random sentence for quote mode, as a list of words."
  @spec quote_words() :: [String.t()]
  def quote_words do
    @sentences |> Enum.random() |> String.split()
  end

  @doc """
  Generates `count` words from `list`, applying the `:punctuation` and
  `:numbers` options.

  Consecutive duplicates are avoided so the text reads like prose rather than
  a stutter. When punctuation is on, the first word is capitalized and every
  sentence-ending mark capitalizes the word that follows it.
  """
  @spec generate(String.t(), pos_integer(), keyword()) :: [String.t()]
  def generate(list, count, opts \\ []) do
    vocab = vocabulary(list) || @english

    vocab
    |> random_words(count)
    |> decorate(opts)
  end

  defp random_words(vocab, count) do
    Enum.map_reduce(1..count, nil, fn _, previous ->
      word = draw(vocab, previous)
      {word, word}
    end)
    |> elem(0)
  end

  defp draw(vocab, previous) do
    case Enum.random(vocab) do
      ^previous -> draw(vocab, previous)
      word -> word
    end
  end

  defp decorate(words, opts) do
    numbers? = Keyword.get(opts, :numbers, false)
    punctuation? = Keyword.get(opts, :punctuation, false)

    words
    |> maybe(numbers?, &add_numbers/1)
    |> maybe(punctuation?, &add_punctuation/1)
  end

  defp maybe(words, true, fun), do: fun.(words)
  defp maybe(words, false, _fun), do: words

  defp add_numbers(words) do
    Enum.map(words, fn word ->
      if :rand.uniform() < 0.12 do
        digits = Enum.random(1..4)
        Integer.to_string(Enum.random(0..(Integer.pow(10, digits) - 1)))
      else
        word
      end
    end)
  end

  # Walks the list carrying "does the next word start a sentence?" so capitals
  # land where a reader would expect them.
  defp add_punctuation(words) do
    words
    |> Enum.map_reduce(true, fn word, sentence_start? ->
      {word, ends_sentence?} = punctuate(word)
      word = if sentence_start?, do: capitalize(word), else: word
      {word, ends_sentence?}
    end)
    |> elem(0)
  end

  defp punctuate(word) do
    roll = :rand.uniform()

    cond do
      roll < 0.06 -> {wrap(word), false}
      roll < 0.10 -> {word <> "'s", false}
      roll < 0.30 -> append_mark(word)
      true -> {word, false}
    end
  end

  defp append_mark(word) do
    mark = Enum.random(@punctuation_marks)
    {word <> mark, mark in [".", "!", "?"]}
  end

  defp wrap(word) do
    {open, close} = Enum.random([{"(", ")"}, {"\"", "\""}, {"'", "'"}, {"[", "]"}])
    open <> word <> close
  end

  # Capitalizes the first letter even when the word opens with a bracket or
  # quote, and leaves the rest of the word alone.
  defp capitalize(word) do
    case String.next_grapheme(word) do
      {opener, rest} when opener in ["(", "\"", "'", "["] -> opener <> upcase_first(rest)
      _ -> upcase_first(word)
    end
  end

  defp upcase_first(word) do
    case String.next_grapheme(word) do
      {first, rest} -> String.upcase(first) <> rest
      nil -> word
    end
  end
end
